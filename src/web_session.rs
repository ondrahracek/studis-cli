//! User-driven browser login and noninteractive read-only session reuse.

use directories::ProjectDirs;
use headless_chrome::{
    Browser, LaunchOptions,
    protocol::cdp::{Browser as CdpBrowser, Network, types::Method},
};
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use std::{
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

use crate::moodle_url;

const MAX_PAGE_BYTES: usize = 2 * 1024 * 1024;
const STUDIS_LOGIN_URL: &str = "https://www.vut.cz/studis/student.phtml";
const MOODLE_LOGIN_URL: &str = "https://moodle.vut.cz/my/";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum LoginTarget {
    #[default]
    Studis,
    Moodle,
}

impl LoginTarget {
    fn url(self) -> &'static str {
        match self {
            Self::Studis => STUDIS_LOGIN_URL,
            Self::Moodle => MOODLE_LOGIN_URL,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WebError {
    AuthRequired,
    BrowserUnavailable,
    ProfileUnsafe,
    PageUnavailable,
    UnexpectedPage,
}

#[derive(Clone)]
pub(crate) struct WebCookie {
    pub(crate) name: String,
    pub(crate) value: String,
    pub(crate) domain: String,
    pub(crate) path: String,
    pub(crate) secure: bool,
}

// headless_chrome 1.0.22 generates this Storage response as one Cookie even
// though the bundled CDP schema defines an array, so keep the adapter local.
#[derive(Debug, Serialize)]
struct GetBrowserCookies {
    #[serde(skip_serializing_if = "Option::is_none", rename = "browserContextId")]
    browser_context_id: Option<CdpBrowser::BrowserContextID>,
}

#[derive(Debug, Deserialize)]
struct GetBrowserCookiesReturn {
    cookies: Vec<Network::Cookie>,
}

impl Method for GetBrowserCookies {
    const NAME: &'static str = "Storage.getCookies";
    type ReturnObject = GetBrowserCookiesReturn;
}

fn retain_moodle_cookie_domains(cookies: Vec<WebCookie>) -> Vec<WebCookie> {
    cookies
        .into_iter()
        .filter(|cookie| {
            let domain = cookie
                .domain
                .strip_prefix('.')
                .unwrap_or(&cookie.domain)
                .to_ascii_lowercase();
            domain == moodle_url::HOST
                || moodle_url::HOST
                    .strip_suffix(&domain)
                    .is_some_and(|prefix| prefix.ends_with('.'))
        })
        .collect()
}

fn cookie_is_unpartitioned(has_partition_key: bool, partition_key_opaque: Option<bool>) -> bool {
    !has_partition_key && partition_key_opaque != Some(true)
}

#[cfg(test)]
impl WebCookie {
    pub(crate) fn synthetic(
        name: &str,
        value: &str,
        domain: &str,
        path: &str,
        secure: bool,
    ) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
            domain: domain.into(),
            path: path.into(),
            secure,
        }
    }
}

impl WebError {
    pub(crate) fn reason(self) -> &'static str {
        match self {
            Self::AuthRequired => "auth_required",
            Self::BrowserUnavailable => "browser_unavailable",
            Self::ProfileUnsafe => "web_profile_unsafe",
            Self::PageUnavailable => "web_fetch_failed",
            Self::UnexpectedPage => "unexpected_web_page",
        }
    }

    pub(crate) fn message(self) -> &'static str {
        match self {
            Self::AuthRequired => "VUT web login is required",
            Self::BrowserUnavailable => "Chrome or Chromium browser is unavailable",
            Self::ProfileUnsafe => "VUT web profile directory is unsafe",
            Self::PageUnavailable => "unable to read VUT web page",
            Self::UnexpectedPage => "unexpected VUT web page",
        }
    }
}

fn profile_path() -> Result<PathBuf, WebError> {
    if let Some(path) = std::env::var_os("STUDIS_WEB_PROFILE_DIR") {
        let path = PathBuf::from(path);
        return path
            .is_absolute()
            .then_some(path)
            .ok_or(WebError::ProfileUnsafe);
    }
    let project = ProjectDirs::from("", "", "studis-cli").ok_or(WebError::ProfileUnsafe)?;
    #[cfg(target_os = "linux")]
    let base = project.state_dir().unwrap_or(project.data_local_dir());
    #[cfg(not(target_os = "linux"))]
    let base = project.data_local_dir();
    Ok(base.join("web-profile"))
}

fn prepare_profile(path: &Path, create: bool) -> Result<(), WebError> {
    #[cfg(unix)]
    validate_profile_ancestors(path)?;
    if !path.exists() && !create {
        return Err(WebError::AuthRequired);
    }
    if create {
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(path)
                .map_err(|_| WebError::ProfileUnsafe)?;
        }
        #[cfg(not(unix))]
        fs::create_dir_all(path).map_err(|_| WebError::ProfileUnsafe)?;
    }
    let metadata = fs::symlink_metadata(path).map_err(|_| WebError::ProfileUnsafe)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(WebError::ProfileUnsafe);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        if metadata.uid() != rustix::process::geteuid().as_raw()
            || metadata.permissions().mode() & 0o777 != 0o700
        {
            return Err(WebError::ProfileUnsafe);
        }
    }
    Ok(())
}

#[cfg(unix)]
fn validate_profile_ancestors(path: &Path) -> Result<(), WebError> {
    use rustix::fs::{self as unix_fs, Mode, OFlags};
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    let owner = rustix::process::geteuid().as_raw();
    for ancestor in path.ancestors().skip(1) {
        let directory = match unix_fs::open(
            ancestor,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        ) {
            Ok(directory) => fs::File::from(directory),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => return Err(WebError::ProfileUnsafe),
        };
        let metadata = directory.metadata().map_err(|_| WebError::ProfileUnsafe)?;
        if (metadata.uid() != owner && metadata.uid() != 0)
            || metadata.permissions().mode() & 0o022 != 0
            || !crate::unix_acl::descriptor_has_no_allow_acl(&directory)
                .map_err(|_| WebError::ProfileUnsafe)?
        {
            return Err(WebError::ProfileUnsafe);
        }
    }
    Ok(())
}

fn browser_options(profile: PathBuf, headed: bool) -> Result<LaunchOptions<'static>, WebError> {
    let path = std::env::var_os("STUDIS_BROWSER_PATH").map(PathBuf::from);
    if path.as_ref().is_some_and(|path| !path.is_file()) {
        return Err(WebError::BrowserUnavailable);
    }
    LaunchOptions::default_builder()
        .headless(!headed)
        .path(path)
        .user_data_dir(Some(profile))
        .ignore_certificate_errors(false)
        .args(vec![OsStr::new("--remote-debugging-address=127.0.0.1")])
        .build()
        .map_err(|_| WebError::BrowserUnavailable)
}

fn validate_final_url(expected: &str, actual: &str) -> Result<(), WebError> {
    let expected = reqwest::Url::parse(expected).map_err(|_| WebError::UnexpectedPage)?;
    let actual = reqwest::Url::parse(actual).map_err(|_| WebError::UnexpectedPage)?;
    if actual.host_str() == Some("id.vut.cz") || actual.path().contains("/login") {
        return Err(WebError::AuthRequired);
    }
    if let Some(expected_course) = moodle_url::canonical_course_url(expected.as_str()) {
        return match moodle_url::canonical_course_url(actual.as_str()) {
            Some(actual_course) if actual_course == expected_course => Ok(()),
            _ => Err(WebError::UnexpectedPage),
        };
    }
    if actual.scheme() != "https"
        || actual.host_str() != expected.host_str()
        || actual.port_or_known_default() != expected.port_or_known_default()
        || actual.path() != expected.path()
    {
        return Err(WebError::UnexpectedPage);
    }
    for (key, value) in expected.query_pairs() {
        if !actual
            .query_pairs()
            .any(|(actual_key, actual_value)| actual_key == key && actual_value == value)
        {
            return Err(WebError::UnexpectedPage);
        }
    }
    Ok(())
}

fn is_authenticated_page(target: LoginTarget, url: &str, html: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(url) else {
        return false;
    };
    match target {
        LoginTarget::Studis => {
            url.scheme() == "https"
                && url.host_str() == Some("www.vut.cz")
                && url.port().is_none()
                && url.path() == "/studis/student.phtml"
                && html.contains("vut-main-content")
        }
        LoginTarget::Moodle => {
            let trusted_origin = url.scheme() == "https"
                && url.host_str() == Some("moodle.vut.cz")
                && url.port().is_none();
            trusted_origin
                && ((matches!(url.path(), "/my/" | "/my/index.php")
                    && (html.contains("id=\"page-my-index\"")
                        || html.contains("id='page-my-index'")))
                    || (url.path() == "/local/customfrontpage/index.php"
                        && authenticated_custom_frontpage(html)))
        }
    }
}

fn authenticated_custom_frontpage(html: &str) -> bool {
    let document = Html::parse_document(html);
    let body = Selector::parse("body#page-local-customfrontpage-index")
        .expect("fixed Moodle frontpage selector");
    if document.select(&body).next().is_none() {
        return false;
    }
    let anchors = Selector::parse("a[href]").expect("fixed Moodle link selector");
    let base = reqwest::Url::parse(MOODLE_LOGIN_URL).expect("fixed Moodle login URL");
    let mut logout = false;
    for href in document
        .select(&anchors)
        .filter_map(|anchor| anchor.value().attr("href"))
        .filter_map(|href| base.join(href).ok())
        .filter(|url| {
            url.scheme() == "https"
                && url.host_str() == Some("moodle.vut.cz")
                && url.port().is_none()
        })
    {
        if href.path() == "/login/index.php" {
            return false;
        }
        logout |= href.path() == "/login/logout.php";
    }
    logout
}

pub(crate) struct WebSession {
    browser: Browser,
}

impl WebSession {
    pub(crate) fn open(headed: bool, create: bool) -> Result<Self, WebError> {
        let profile = profile_path()?;
        prepare_profile(&profile, create)?;
        let options = browser_options(profile, headed)?;
        let browser = Browser::new(options).map_err(|_| WebError::BrowserUnavailable)?;
        Ok(Self { browser })
    }

    fn navigate(&self, url: &str) -> Result<(String, String), WebError> {
        let tab = self
            .browser
            .new_tab()
            .map_err(|_| WebError::PageUnavailable)?;
        tab.navigate_to(url)
            .map_err(|_| WebError::PageUnavailable)?;
        tab.wait_until_navigated()
            .map_err(|_| WebError::PageUnavailable)?;
        let final_url = tab.get_url();
        validate_final_url(url, &final_url)?;
        let html = tab.get_content().map_err(|_| WebError::PageUnavailable)?;
        if html.len() > MAX_PAGE_BYTES {
            return Err(WebError::PageUnavailable);
        }
        Ok((final_url, html))
    }

    pub(crate) fn read(&self, url: &str) -> Result<String, WebError> {
        self.navigate(url).map(|(_, html)| html)
    }

    pub(crate) fn cookies_for(&self, url: &str) -> Result<Vec<WebCookie>, WebError> {
        let tab = self
            .browser
            .new_tab()
            .map_err(|_| WebError::PageUnavailable)?;
        tab.navigate_to(url)
            .map_err(|_| WebError::PageUnavailable)?;
        tab.wait_until_navigated()
            .map_err(|_| WebError::PageUnavailable)?;
        validate_final_url(url, &tab.get_url())?;
        let html = tab.get_content().map_err(|_| WebError::PageUnavailable)?;
        if html.len() > MAX_PAGE_BYTES {
            return Err(WebError::PageUnavailable);
        }
        tab.call_method(GetBrowserCookies {
            browser_context_id: None,
        })
        .map_err(|_| WebError::PageUnavailable)
        .map(|result| {
            retain_moodle_cookie_domains(
                result
                    .cookies
                    .into_iter()
                    .filter(|cookie| {
                        cookie_is_unpartitioned(
                            cookie.partition_key.is_some(),
                            cookie.partition_key_opaque,
                        )
                    })
                    .map(|cookie| WebCookie {
                        name: cookie.name,
                        value: cookie.value,
                        domain: cookie.domain,
                        path: cookie.path,
                        secure: cookie.secure,
                    })
                    .collect(),
            )
        })
    }

    pub(crate) fn login(&self, target: LoginTarget) -> Result<(), WebError> {
        let tab = self
            .browser
            .new_tab()
            .map_err(|_| WebError::PageUnavailable)?;
        tab.navigate_to(target.url())
            .map_err(|_| WebError::PageUnavailable)?;
        tab.wait_until_navigated()
            .map_err(|_| WebError::PageUnavailable)?;
        let deadline = Instant::now() + Duration::from_secs(300);
        loop {
            let url = tab.get_url();
            if tab
                .get_content()
                .is_ok_and(|html| is_authenticated_page(target, &url, &html))
            {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(WebError::AuthRequired);
            }
            thread::sleep(Duration::from_millis(500));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_options_keep_certificate_checks_on_and_isolate_profile() {
        let profile = std::env::temp_dir().join("studis-synthetic-profile");
        let options = browser_options(profile.clone(), false).expect("options");
        assert!(!options.ignore_certificate_errors);
        assert!(options.headless);
        assert_eq!(options.user_data_dir, Some(profile));
    }

    #[test]
    fn redirect_validation_rejects_login_and_wrong_course() {
        let expected = "https://moodle.vut.cz/course/view.php?id=42";
        assert_eq!(validate_final_url(expected, expected), Ok(()));
        assert_eq!(
            validate_final_url(expected, "https://moodle.vut.cz/course/view.php?id=43"),
            Err(WebError::UnexpectedPage)
        );
        for actual in [
            "https://moodle.vut.cz/course/view.php?id=42&id=43",
            "https://moodle.vut.cz/course/view.php?id=42&token=extra",
        ] {
            assert_eq!(
                validate_final_url(expected, actual),
                Err(WebError::UnexpectedPage),
                "accepted {actual}"
            );
        }
        assert_eq!(
            validate_final_url(expected, "https://moodle.vut.cz:444/course/view.php?id=42"),
            Err(WebError::UnexpectedPage)
        );
        assert_eq!(
            validate_final_url(expected, "https://id.vut.cz/auth/common/home/default"),
            Err(WebError::AuthRequired)
        );
        assert_eq!(
            validate_final_url(expected, "https://moodle.vut.cz/login/index.php"),
            Err(WebError::AuthRequired)
        );
        assert_eq!(
            validate_final_url(
                "https://www.vut.cz/studis/student.phtml?gm=detail",
                "https://www.vut.cz/studis/student.phtml?gm=detail&navigation=1"
            ),
            Ok(())
        );
    }

    #[test]
    fn login_targets_require_their_own_authenticated_page_marker() {
        assert!(is_authenticated_page(
            LoginTarget::Studis,
            "https://www.vut.cz/studis/student.phtml",
            "<body><main class='vut-main-content'></main></body>"
        ));
        assert!(is_authenticated_page(
            LoginTarget::Moodle,
            "https://moodle.vut.cz/my/",
            "<body id='page-my-index' class='pagelayout-mydashboard'></body>"
        ));
        assert!(is_authenticated_page(
            LoginTarget::Moodle,
            "https://moodle.vut.cz/local/customfrontpage/index.php",
            "<body id='page-local-customfrontpage-index'><a href='/login/logout.php?sesskey=synthetic'>Log out</a></body>"
        ));
        assert!(!is_authenticated_page(
            LoginTarget::Moodle,
            "https://moodle.vut.cz/local/customfrontpage/index.php",
            "<body id='page-local-customfrontpage-index'><a href='/login/index.php'>Log in</a></body>"
        ));
        assert!(!is_authenticated_page(
            LoginTarget::Moodle,
            "https://moodle.vut.cz/login/index.php",
            "<body id='page-login-index'></body>"
        ));
        assert!(!is_authenticated_page(
            LoginTarget::Moodle,
            "https://moodle.vut.cz/my/",
            "<body><main>Generic page</main></body>"
        ));
        assert!(!is_authenticated_page(
            LoginTarget::Moodle,
            "https://moodle.vut.cz:444/my/",
            "<body id='page-my-index' class='pagelayout-mydashboard'></body>"
        ));
        assert!(!is_authenticated_page(
            LoginTarget::Studis,
            "https://www.vut.cz:444/studis/student.phtml",
            "<body><main class='vut-main-content'></main></body>"
        ));
    }

    #[test]
    fn browser_cookie_prefilter_keeps_only_domains_that_can_match_moodle() {
        let cookies = vec![
            WebCookie::synthetic("host", "one", "moodle.vut.cz", "/", true),
            WebCookie::synthetic("parent", "two", ".vut.cz", "/", true),
            WebCookie::synthetic("subdomain", "three", ".moodle.vut.cz", "/", true),
            WebCookie::synthetic("sibling", "four", "www.vut.cz", "/", true),
            WebCookie::synthetic("foreign", "five", ".evil.example", "/", true),
        ];
        assert_eq!(
            retain_moodle_cookie_domains(cookies)
                .into_iter()
                .map(|cookie| cookie.name)
                .collect::<Vec<_>>(),
            ["host", "parent", "subdomain"]
        );
    }

    #[test]
    fn browser_cookie_prefilter_rejects_partitioned_contexts() {
        assert!(cookie_is_unpartitioned(false, None));
        assert!(cookie_is_unpartitioned(false, Some(false)));
        assert!(!cookie_is_unpartitioned(true, None));
        assert!(!cookie_is_unpartitioned(true, Some(false)));
        assert!(!cookie_is_unpartitioned(false, Some(true)));
        assert!(!cookie_is_unpartitioned(true, Some(true)));
    }

    #[cfg(unix)]
    #[test]
    fn browser_profile_rejects_shared_ancestors() {
        let shared_parent = std::env::temp_dir();
        assert_eq!(
            validate_profile_ancestors(&shared_parent.join("studis-profile")),
            Err(WebError::ProfileUnsafe)
        );
    }

    #[cfg(unix)]
    #[test]
    fn default_browser_profile_has_private_ancestors() {
        let profile = profile_path().expect("default profile path");
        assert_eq!(validate_profile_ancestors(&profile), Ok(()));
    }
}
