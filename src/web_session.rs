//! User-driven browser login and noninteractive read-only session reuse.

use directories::ProjectDirs;
use headless_chrome::{Browser, LaunchOptions};
use std::{
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

const MAX_PAGE_BYTES: usize = 2 * 1024 * 1024;
const LOGIN_URL: &str = "https://www.vut.cz/studis/student.phtml";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WebError {
    AuthRequired,
    BrowserUnavailable,
    ProfileUnsafe,
    PageUnavailable,
    UnexpectedPage,
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
    if actual.scheme() != "https"
        || actual.host_str() != expected.host_str()
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

    pub(crate) fn login(&self) -> Result<(), WebError> {
        let tab = self
            .browser
            .new_tab()
            .map_err(|_| WebError::PageUnavailable)?;
        tab.navigate_to(LOGIN_URL)
            .map_err(|_| WebError::PageUnavailable)?;
        tab.wait_until_navigated()
            .map_err(|_| WebError::PageUnavailable)?;
        let deadline = Instant::now() + Duration::from_secs(300);
        loop {
            let url = tab.get_url();
            if url.starts_with(LOGIN_URL)
                && tab
                    .get_content()
                    .is_ok_and(|html| html.contains("vut-main-content"))
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
        assert_eq!(
            validate_final_url(expected, "https://id.vut.cz/auth/common/home/default"),
            Err(WebError::AuthRequired)
        );
        assert_eq!(
            validate_final_url(expected, "https://moodle.vut.cz/login/index.php"),
            Err(WebError::AuthRequired)
        );
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
