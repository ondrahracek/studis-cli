//! One-resource Moodle downloads with scoped browser cookies and atomic output.

use reqwest::{
    StatusCode, Url,
    blocking::{Client, Response},
    header::{CONTENT_LENGTH, CONTENT_RANGE, CONTENT_TYPE, COOKIE, LOCATION},
};
use serde::Serialize;
use std::{
    ffi::OsString,
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use crate::{
    moodle_files, moodle_url,
    subject_view::SubjectRequest,
    web::moodle::{self, PageError},
    web_session::{WebCookie, WebSession},
};

const MAX_DOWNLOAD_BYTES: u64 = 100 * 1024 * 1024;
const MAX_HTML_BYTES: u64 = 8 * 1024 * 1024;
const MAX_REDIRECTS: usize = 5;
static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, PartialEq, Eq)]
enum DownloadError {
    AuthRequired,
    PermissionDenied,
    RateLimited,
    UnexpectedPage,
    FetchFailed,
    TooLarge,
    HtmlTooLarge,
    OutputExists,
    OutputInvalid,
    Listing(String),
}

impl DownloadError {
    fn message(&self) -> &str {
        match self {
            Self::AuthRequired => "VUT web login is required",
            Self::PermissionDenied => "Moodle resource access denied",
            Self::RateLimited => "Moodle rate limited the request",
            Self::UnexpectedPage => "unexpected Moodle download page",
            Self::FetchFailed => "unable to download Moodle resource",
            Self::TooLarge => "Moodle resource exceeds the 100 MiB download limit",
            Self::HtmlTooLarge => "HTML resource exceeds the 8 MiB inspection limit",
            Self::OutputExists => "output path already exists",
            Self::OutputInvalid => "output path is unavailable",
            Self::Listing(message) => message,
        }
    }
}

#[derive(Serialize)]
pub(crate) struct DownloadReceipt {
    schema_version: u8,
    status: &'static str,
    module_id: u64,
    output: String,
    bytes: u64,
}

fn login_url(url: &Url) -> bool {
    url.host_str() == Some("id.vut.cz")
        || (url.host_str() == Some(moodle_url::HOST)
            && (url.path() == "/login/index.php" || url.path().starts_with("/login/")))
}

fn redirect_target(current: &Url, location: &str) -> Result<Url, DownloadError> {
    let target = current
        .join(location)
        .map_err(|_| DownloadError::UnexpectedPage)?;
    if login_url(&target) {
        return Err(DownloadError::AuthRequired);
    }
    if !moodle_url::has_exact_origin(&target) {
        return Err(DownloadError::UnexpectedPage);
    }
    Ok(target)
}

fn cookie_name_is_safe(name: &str) -> bool {
    !name.is_empty()
        && name.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(
                    byte,
                    b'!' | b'#'
                        | b'$'
                        | b'%'
                        | b'&'
                        | b'\''
                        | b'*'
                        | b'+'
                        | b'-'
                        | b'.'
                        | b'^'
                        | b'_'
                        | b'`'
                        | b'|'
                        | b'~'
                )
        })
}

fn cookie_value_is_safe(value: &str) -> bool {
    value
        .bytes()
        .all(|byte| (0x21..=0x7e).contains(&byte) && byte != b';')
}

fn cookie_matches(cookie: &WebCookie, target: &Url) -> bool {
    if !moodle_url::has_exact_origin(target)
        || !cookie_name_is_safe(&cookie.name)
        || !cookie_value_is_safe(&cookie.value)
        || (cookie.secure && target.scheme() != "https")
    {
        return false;
    }
    let Some(host) = target.host_str() else {
        return false;
    };
    let domain = cookie.domain.strip_prefix('.').unwrap_or(&cookie.domain);
    let domain_matches = if cookie.domain.starts_with('.') {
        host == domain || host.ends_with(&format!(".{domain}"))
    } else {
        host == domain
    };
    let cookie_path = cookie.path.as_str();
    let request_path = target.path();
    let path_matches = cookie_path.starts_with('/')
        && (request_path == cookie_path
            || (request_path.starts_with(cookie_path)
                && (cookie_path.ends_with('/')
                    || request_path.as_bytes().get(cookie_path.len()) == Some(&b'/'))));
    domain_matches && path_matches
}

fn cookie_header(cookies: &[WebCookie], target: &Url) -> Option<String> {
    let value = cookies
        .iter()
        .filter(|cookie| cookie_matches(cookie, target))
        .map(|cookie| format!("{}={}", cookie.name, cookie.value))
        .collect::<Vec<_>>()
        .join("; ");
    (!value.is_empty()).then_some(value)
}

fn classify_html_payload(content_type: Option<&str>, prefix: &[u8]) -> Result<(), DownloadError> {
    if !content_type.is_some_and(|value| value.to_ascii_lowercase().starts_with("text/html")) {
        return Ok(());
    }
    match std::str::from_utf8(prefix)
        .ok()
        .and_then(moodle::page_error)
    {
        Some(PageError::AuthRequired) => Err(DownloadError::AuthRequired),
        Some(PageError::PermissionDenied) => Err(DownloadError::PermissionDenied),
        None => Ok(()),
    }
}

fn is_html_content_type(content_type: Option<&str>) -> bool {
    content_type.is_some_and(|value| value.to_ascii_lowercase().starts_with("text/html"))
}

fn validate_payload_status(
    status: StatusCode,
    has_content_range: bool,
) -> Result<(), DownloadError> {
    if status == StatusCode::OK && !has_content_range {
        Ok(())
    } else {
        Err(DownloadError::UnexpectedPage)
    }
}

struct TemporaryOutput {
    path: PathBuf,
    file: File,
    published: bool,
}

impl Drop for TemporaryOutput {
    fn drop(&mut self) {
        if !self.published {
            let _ = fs::remove_file(&self.path);
        }
    }
}

fn output_parent(output: &Path) -> Result<&Path, DownloadError> {
    let parent = output.parent().unwrap_or_else(|| Path::new("."));
    let parent = if parent.as_os_str().is_empty() {
        Path::new(".")
    } else {
        parent
    };
    output
        .file_name()
        .filter(|name| !name.is_empty())
        .ok_or(DownloadError::OutputInvalid)?;
    if !fs::metadata(parent)
        .map_err(|_| DownloadError::OutputInvalid)?
        .is_dir()
    {
        return Err(DownloadError::OutputInvalid);
    }
    Ok(parent)
}

fn create_temporary(output: &Path) -> Result<TemporaryOutput, DownloadError> {
    match fs::symlink_metadata(output) {
        Ok(_) => return Err(DownloadError::OutputExists),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err(DownloadError::OutputInvalid),
    }
    let parent = output_parent(output)?;
    for _ in 0..32 {
        let mut name = OsString::from(".");
        name.push(output.file_name().expect("validated output filename"));
        name.push(format!(
            ".studis-{}-{}.tmp",
            std::process::id(),
            NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed)
        ));
        let path = parent.join(name);
        let mut options = OpenOptions::new();
        options.read(true).write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(&path) {
            Ok(file) => {
                return Ok(TemporaryOutput {
                    path,
                    file,
                    published: false,
                });
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(_) => return Err(DownloadError::OutputInvalid),
        }
    }
    Err(DownloadError::OutputInvalid)
}

#[cfg(unix)]
fn publish_temporary(temporary: &Path, output: &Path) -> Result<(), DownloadError> {
    rustix::fs::renameat_with(
        rustix::fs::CWD,
        temporary,
        rustix::fs::CWD,
        output,
        rustix::fs::RenameFlags::NOREPLACE,
    )
    .map_err(|error| {
        if error.kind() == std::io::ErrorKind::AlreadyExists {
            DownloadError::OutputExists
        } else {
            DownloadError::OutputInvalid
        }
    })
}

#[cfg(not(unix))]
fn publish_temporary(temporary: &Path, output: &Path) -> Result<(), DownloadError> {
    fs::hard_link(temporary, output).map_err(|error| {
        if error.kind() == std::io::ErrorKind::AlreadyExists {
            DownloadError::OutputExists
        } else {
            DownloadError::OutputInvalid
        }
    })?;
    fs::remove_file(temporary).map_err(|_| DownloadError::OutputInvalid)
}

fn write_temporary(
    mut reader: impl Read,
    output: &Path,
    max_bytes: u64,
) -> Result<(TemporaryOutput, u64), DownloadError> {
    let mut temporary = create_temporary(output)?;
    let mut total = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|_| DownloadError::FetchFailed)?;
        if read == 0 {
            break;
        }
        total = total
            .checked_add(read as u64)
            .ok_or(DownloadError::TooLarge)?;
        if total > max_bytes {
            return Err(DownloadError::TooLarge);
        }
        temporary
            .file
            .write_all(&buffer[..read])
            .map_err(|_| DownloadError::OutputInvalid)?;
    }
    Ok((temporary, total))
}

fn publish_output(
    mut temporary: TemporaryOutput,
    output: &Path,
    total: u64,
) -> Result<u64, DownloadError> {
    temporary
        .file
        .sync_all()
        .map_err(|_| DownloadError::OutputInvalid)?;
    publish_temporary(&temporary.path, output)?;
    temporary.published = true;
    Ok(total)
}

fn store_reader(reader: impl Read, output: &Path, max_bytes: u64) -> Result<u64, DownloadError> {
    let (temporary, total) = write_temporary(reader, output, max_bytes)?;
    publish_output(temporary, output, total)
}

fn store_html_reader(reader: impl Read, output: &Path) -> Result<u64, DownloadError> {
    let (mut temporary, total) = match write_temporary(reader, output, MAX_HTML_BYTES) {
        Err(DownloadError::TooLarge) => return Err(DownloadError::HtmlTooLarge),
        result => result?,
    };
    temporary
        .file
        .seek(SeekFrom::Start(0))
        .map_err(|_| DownloadError::OutputInvalid)?;
    let mut html = Vec::with_capacity(total as usize);
    (&mut temporary.file)
        .take(total + 1)
        .read_to_end(&mut html)
        .map_err(|_| DownloadError::OutputInvalid)?;
    if html.len() as u64 != total {
        return Err(DownloadError::OutputInvalid);
    }
    classify_html_payload(Some("text/html"), &html)?;
    publish_output(temporary, output, total)
}

fn response_for(
    client: &Client,
    cookies: &[WebCookie],
    initial: Url,
) -> Result<Response, DownloadError> {
    let mut current = initial;
    if !moodle_url::has_exact_origin(&current) {
        return Err(DownloadError::UnexpectedPage);
    }
    for redirects in 0..=MAX_REDIRECTS {
        let mut request = client.get(current.clone());
        if let Some(value) = cookie_header(cookies, &current) {
            request = request.header(COOKIE, value);
        }
        let response = request.send().map_err(|_| DownloadError::FetchFailed)?;
        if response.status().is_redirection() {
            if redirects == MAX_REDIRECTS {
                return Err(DownloadError::UnexpectedPage);
            }
            let location = response
                .headers()
                .get(LOCATION)
                .and_then(|value| value.to_str().ok())
                .ok_or(DownloadError::UnexpectedPage)?;
            current = redirect_target(&current, location)?;
            continue;
        }
        return match response.status() {
            StatusCode::UNAUTHORIZED => Err(DownloadError::AuthRequired),
            StatusCode::FORBIDDEN => Err(DownloadError::PermissionDenied),
            StatusCode::TOO_MANY_REQUESTS => Err(DownloadError::RateLimited),
            status if status.is_success() => Ok(response),
            _ => Err(DownloadError::FetchFailed),
        };
    }
    Err(DownloadError::UnexpectedPage)
}

fn run(
    request: SubjectRequest,
    module_id: u64,
    output: &Path,
) -> Result<DownloadReceipt, DownloadError> {
    match fs::symlink_metadata(output) {
        Ok(_) => return Err(DownloadError::OutputExists),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err(DownloadError::OutputInvalid),
    }
    output_parent(output)?;
    let source =
        moodle_files::fetch_resource(request, module_id).map_err(DownloadError::Listing)?;
    let initial = Url::parse(&source.activity_url).map_err(|_| DownloadError::UnexpectedPage)?;
    let canonical_activity = moodle_url::canonical_activity_url(&initial, "resource", module_id)
        .ok_or(DownloadError::UnexpectedPage)?;
    if initial.as_str() != canonical_activity {
        return Err(DownloadError::UnexpectedPage);
    }
    let session = WebSession::open(false, false).map_err(|error| match error.reason() {
        "auth_required" => DownloadError::AuthRequired,
        _ => DownloadError::FetchFailed,
    })?;
    let cookies =
        session
            .cookies_for(&source.course_url)
            .map_err(|error| match error.reason() {
                "auth_required" => DownloadError::AuthRequired,
                "unexpected_web_page" => DownloadError::UnexpectedPage,
                _ => DownloadError::FetchFailed,
            })?;
    let client = Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(120))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| DownloadError::FetchFailed)?;
    let response = response_for(&client, &cookies, initial)?;
    validate_payload_status(
        response.status(),
        response.headers().contains_key(CONTENT_RANGE),
    )?;
    if !moodle_url::is_payload_url(response.url()) {
        return Err(DownloadError::UnexpectedPage);
    }
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let html = is_html_content_type(content_type.as_deref());
    let content_length = response
        .headers()
        .get(CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or_default();
    if html && content_length > MAX_HTML_BYTES {
        return Err(DownloadError::HtmlTooLarge);
    }
    if content_length > MAX_DOWNLOAD_BYTES {
        return Err(DownloadError::TooLarge);
    }
    let bytes = if html {
        store_html_reader(response, output)?
    } else {
        store_reader(response, output, MAX_DOWNLOAD_BYTES)?
    };
    Ok(DownloadReceipt {
        schema_version: 1,
        status: "downloaded",
        module_id,
        output: output.to_string_lossy().into_owned(),
        bytes,
    })
}

pub(crate) fn download(
    request: SubjectRequest,
    module_id: u64,
    output: &Path,
) -> Result<DownloadReceipt, String> {
    run(request, module_id, output).map_err(|error| error.message().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::web_session::WebCookie;
    use std::{fs, io::Cursor, path::PathBuf};

    fn scratch(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "studis-download-{name}-{}-{}",
            std::process::id(),
            NEXT_TEMP_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        fs::create_dir(&path).expect("create scratch directory");
        path
    }

    #[test]
    fn cookies_are_scoped_to_exact_moodle_origin_domain_and_path() {
        let cookies = vec![
            WebCookie::synthetic("root", "one", ".vut.cz", "/", true),
            WebCookie::synthetic("moodle", "two", "moodle.vut.cz", "/mod", true),
            WebCookie::synthetic("wrong_path", "three", "moodle.vut.cz", "/my", true),
            WebCookie::synthetic("wrong_domain", "four", ".evil.example", "/", true),
        ];
        let target =
            reqwest::Url::parse("https://moodle.vut.cz/mod/resource/view.php?id=5").unwrap();

        assert_eq!(
            cookie_header(&cookies, &target).as_deref(),
            Some("root=one; moodle=two")
        );
        for unsafe_url in [
            "http://moodle.vut.cz/mod/resource/view.php?id=5",
            "https://moodle.vut.cz:444/mod/resource/view.php?id=5",
            "https://evil.example/mod/resource/view.php?id=5",
        ] {
            let unsafe_url = reqwest::Url::parse(unsafe_url).unwrap();
            assert!(cookie_header(&cookies, &unsafe_url).is_none());
        }
    }

    #[test]
    fn redirects_stay_on_the_moodle_origin_and_login_is_distinct() {
        let current =
            reqwest::Url::parse("https://moodle.vut.cz/mod/resource/view.php?id=5").unwrap();
        assert_eq!(
            redirect_target(&current, "/pluginfile.php/1/file.pdf")
                .unwrap()
                .as_str(),
            "https://moodle.vut.cz/pluginfile.php/1/file.pdf"
        );
        assert_eq!(
            redirect_target(&current, "/login/index.php"),
            Err(DownloadError::AuthRequired)
        );
        assert_eq!(
            redirect_target(&current, "https://id.vut.cz/auth/common/home/default"),
            Err(DownloadError::AuthRequired)
        );
        assert_eq!(
            redirect_target(&current, "https://files.example/file.pdf"),
            Err(DownloadError::UnexpectedPage)
        );
        assert!(moodle_url::is_payload_url(
            &reqwest::Url::parse("https://moodle.vut.cz/pluginfile.php/1/file.pdf").unwrap()
        ));
        assert!(!moodle_url::is_payload_url(
            &reqwest::Url::parse("https://moodle.vut.cz/course/view.php?id=42").unwrap()
        ));
    }

    #[test]
    fn login_html_is_detected_without_accepting_arbitrary_html() {
        assert_eq!(
            classify_html_payload(
                Some("text/html; charset=utf-8"),
                b"<body id='page-login-index'><form action='/login/index.php'></form></body>"
            ),
            Err(DownloadError::AuthRequired)
        );
        assert_eq!(
            classify_html_payload(Some("text/html"), b"<body><p>Course handout</p></body>"),
            Ok(())
        );
        assert_eq!(
            classify_html_payload(Some("application/pdf"), b"<body id='page-login-index'>"),
            Ok(())
        );
    }

    #[test]
    fn payload_requires_a_complete_200_response_without_content_range() {
        assert_eq!(validate_payload_status(StatusCode::OK, false), Ok(()));
        assert_eq!(
            validate_payload_status(StatusCode::PARTIAL_CONTENT, true),
            Err(DownloadError::UnexpectedPage)
        );
        assert_eq!(
            validate_payload_status(StatusCode::PARTIAL_CONTENT, false),
            Err(DownloadError::UnexpectedPage)
        );
        assert_eq!(
            validate_payload_status(StatusCode::OK, true),
            Err(DownloadError::UnexpectedPage)
        );
    }

    #[test]
    fn html_payload_classifier_rejects_moodle_errors_without_rejecting_html_files() {
        assert_eq!(
            classify_html_payload(
                Some("text/html; charset=utf-8"),
                b"<body id='page-login-index'><form action='/login/index.php'></form></body>"
            ),
            Err(DownloadError::AuthRequired)
        );
        for marker in [
            "<div class='errorbox'>Unavailable</div>",
            "<div class='alert alert-danger'>Access denied</div>",
        ] {
            let html = format!("<body id='page-error-general'>{marker}</body>");
            assert_eq!(
                classify_html_payload(Some("text/html"), html.as_bytes()),
                Err(DownloadError::PermissionDenied)
            );
        }
        assert_eq!(
            classify_html_payload(
                Some("text/html"),
                b"<body><article class='alert-danger'>Legitimate course notes</article></body>"
            ),
            Ok(())
        );
        assert_eq!(
            classify_html_payload(
                Some("application/octet-stream"),
                b"<body id='page-error-general'><div class='errorbox'>bytes</div></body>"
            ),
            Ok(())
        );
    }

    #[test]
    fn html_error_after_the_initial_buffer_is_rejected_before_publish() {
        let directory = scratch("late-html-error");
        let output = directory.join("error.html");
        let html = format!(
            "<body id='page-error-general'><p>{}</p><div class='alert-danger'>Denied</div></body>",
            "harmless".repeat(1_200)
        );
        assert!(html.find("alert-danger").unwrap() > 8 * 1024);

        assert_eq!(
            store_html_reader(Cursor::new(html.as_bytes()), &output),
            Err(DownloadError::PermissionDenied)
        );
        assert!(!output.exists());
        assert_eq!(fs::read_dir(&directory).unwrap().count(), 0);

        let legitimate = directory.join("notes.html");
        let html = format!(
            "<body><article>{}</article><p>Legitimate notes</p></body>",
            "content".repeat(1_200)
        );
        let bytes = store_html_reader(Cursor::new(html.as_bytes()), &legitimate).unwrap();
        assert_eq!(bytes, html.len() as u64);
        assert_eq!(fs::read_to_string(&legitimate).unwrap(), html);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn bounded_output_is_private_atomic_and_never_overwrites() {
        let directory = scratch("publish");
        let output = directory.join("handout.bin");
        let bytes = store_reader(Cursor::new(b"four"), &output, 4).unwrap();
        assert_eq!(bytes, 4);
        assert_eq!(fs::read(&output).unwrap(), b"four");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&output).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }

        assert_eq!(
            store_reader(Cursor::new(b"new"), &output, 4),
            Err(DownloadError::OutputExists)
        );
        assert_eq!(fs::read(&output).unwrap(), b"four");
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn oversized_output_is_removed_without_publishing() {
        let directory = scratch("limit");
        let output = directory.join("too-large.bin");
        assert_eq!(
            store_reader(Cursor::new(b"12345"), &output, 4),
            Err(DownloadError::TooLarge)
        );
        assert!(!output.exists());
        assert_eq!(fs::read_dir(&directory).unwrap().count(), 0);
        fs::remove_dir(directory).unwrap();
    }
}
