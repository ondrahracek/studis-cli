//! Bearer-token sourcing for read-only requests.

use reqwest::blocking::{Client, Request};
use reqwest::header::CONTENT_TYPE;

use crate::{
    http,
    token_store::{PlatformTokenStore, TokenStore},
};

pub(crate) const TOKEN_URL: &str = "https://id.vut.cz/auth/common/oauth2/token";

pub(crate) struct Credentials {
    pub(crate) uid: String,
    pub(crate) secret: String,
}

enum AuthSource {
    Token(String),
    ClientCredentials(Credentials),
}

fn select_source(
    token: Option<String>,
    uid: Option<String>,
    secret: Option<String>,
) -> Result<AuthSource, &'static str> {
    match token {
        Some(token) if token.is_empty() => Err("VUT API access token is empty"),
        Some(token) => Ok(AuthSource::Token(token)),
        None => validate_credentials(uid, secret).map(AuthSource::ClientCredentials),
    }
}

fn source_from_env() -> Result<AuthSource, &'static str> {
    let token = std::env::var("VUT_API_ACCESS_TOKEN").ok();
    let (uid, secret) = if token.is_some() {
        (None, None)
    } else {
        (
            std::env::var("VUT_API_CLIENT_UID").ok(),
            std::env::var("VUT_API_CLIENT_SECRET").ok(),
        )
    };
    select_source(token, uid, secret)
}

pub(crate) fn validate_credentials(
    uid: Option<String>,
    secret: Option<String>,
) -> Result<Credentials, &'static str> {
    match (uid, secret) {
        (Some(uid), Some(secret)) if !uid.is_empty() && !secret.is_empty() => {
            Ok(Credentials { uid, secret })
        }
        _ => Err("VUT API credentials are missing"),
    }
}

pub(crate) fn token_request(
    client: &Client,
    credentials: &Credentials,
) -> Result<Request, &'static str> {
    client
        .post(TOKEN_URL)
        .basic_auth(&credentials.uid, Some(&credentials.secret))
        .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body("grant_type=client_credentials")
        .build()
        .map_err(|_| "unable to prepare VUT authentication request")
}

pub(crate) fn parse_access_token(body: &str) -> Result<String, &'static str> {
    let response: serde_json::Value =
        serde_json::from_str(body).map_err(|_| "invalid VUT authentication response")?;
    let token = response
        .get("access_token")
        .and_then(serde_json::Value::as_str)
        .filter(|token| !token.is_empty())
        .ok_or("invalid VUT authentication response")?;
    let token_type = response
        .get("token_type")
        .and_then(serde_json::Value::as_str)
        .ok_or("invalid VUT authentication response")?;
    if !token_type.eq_ignore_ascii_case("Bearer") {
        return Err("invalid VUT authentication response");
    }
    Ok(token.to_owned())
}

fn token_status(status: reqwest::StatusCode) -> Result<(), &'static str> {
    match status {
        reqwest::StatusCode::OK => Ok(()),
        reqwest::StatusCode::TOO_MANY_REQUESTS => Err("VUT authentication rate limited"),
        _ => Err("VUT authentication failed"),
    }
}

fn issue_token(client: &Client, credentials: &Credentials) -> Result<String, &'static str> {
    let request = token_request(client, credentials)?;
    let response = client
        .execute(request)
        .map_err(|_| "unable to contact VUT authentication service")?;
    token_status(response.status())?;
    let body = response
        .text()
        .map_err(|_| "unable to read VUT authentication response")?;
    parse_access_token(&body)
}

fn run_cached<S, I, G>(store: &S, mut issue: I, mut get: G) -> Result<String, &'static str>
where
    S: TokenStore,
    I: FnMut() -> Result<String, &'static str>,
    G: FnMut(&str) -> Result<String, http::GetError>,
{
    let (token, came_from_cache) = match store.load()? {
        Some(token) => (token, true),
        None => store.with_lock(|| match store.load()? {
            Some(token) => Ok((token, true)),
            None => {
                let token = issue()?;
                store.save(&token)?;
                Ok((token, false))
            }
        })?,
    };

    match get(&token) {
        Ok(body) => Ok(body),
        Err(http::GetError::Unauthorized) if came_from_cache => {
            let replacement = store.with_lock(|| match store.load()? {
                Some(newer) if newer != token => Ok(newer),
                _ => {
                    let newer = issue()?;
                    store.save(&newer)?;
                    Ok(newer)
                }
            })?;
            get(&replacement).map_err(|error| error.message())
        }
        Err(error) => Err(error.message()),
    }
}

pub(crate) fn get_body<F>(make_request: F) -> Result<String, &'static str>
where
    F: Fn(&Client, &str) -> Result<Request, &'static str>,
{
    let source = source_from_env()?;
    let client = http::client()?;
    let get = |token: &str| {
        let request = make_request(&client, token).map_err(http::GetError::Other)?;
        http::get_body(&client, request)
    };
    match source {
        AuthSource::Token(token) => get(&token).map_err(|error| error.message()),
        AuthSource::ClientCredentials(credentials) => {
            let store = PlatformTokenStore::new(&credentials.uid)?;
            run_cached(&store, || issue_token(&client, &credentials), get)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::AUTHORIZATION;
    use std::cell::{Cell, RefCell};
    #[cfg(unix)]
    use std::{
        fs::{self, OpenOptions},
        io::Write,
        path::{Path, PathBuf},
        process::{Child, Command, Stdio},
        time::{Duration, Instant, SystemTime, UNIX_EPOCH},
    };

    struct MemoryStore {
        token: RefCell<Option<String>>,
        fail_load: bool,
        fail_save: bool,
        fail_lock: bool,
        locks: Cell<usize>,
    }

    impl MemoryStore {
        fn new(token: Option<&str>) -> Self {
            Self {
                token: RefCell::new(token.map(str::to_owned)),
                fail_load: false,
                fail_save: false,
                fail_lock: false,
                locks: Cell::new(0),
            }
        }
    }

    impl TokenStore for MemoryStore {
        fn load(&self) -> Result<Option<String>, &'static str> {
            if self.fail_load {
                return Err("store read failed");
            }
            Ok(self.token.borrow().clone())
        }

        fn save(&self, token: &str) -> Result<(), &'static str> {
            if self.fail_save {
                return Err("store write failed");
            }
            *self.token.borrow_mut() = Some(token.to_owned());
            Ok(())
        }

        fn with_lock<T>(
            &self,
            operation: impl FnOnce() -> Result<T, &'static str>,
        ) -> Result<T, &'static str> {
            self.locks.set(self.locks.get() + 1);
            if self.fail_lock {
                return Err("store lock failed");
            }
            operation()
        }
    }

    #[test]
    fn cached_token_is_used_without_grant_even_if_old() {
        let store = MemoryStore::new(Some("old-token"));
        let body = run_cached(
            &store,
            || panic!("must not grant"),
            |token| {
                assert_eq!(token, "old-token");
                Ok("response".into())
            },
        )
        .expect("cached GET");
        assert_eq!(body, "response");
    }

    #[test]
    fn cache_miss_grants_and_saves_before_get() {
        let store = MemoryStore::new(None);
        let body = run_cached(
            &store,
            || Ok("new-token".into()),
            |token| {
                assert_eq!(store.token.borrow().as_deref(), Some(token));
                Ok("response".into())
            },
        )
        .expect("new token GET");
        assert_eq!(body, "response");
        assert_eq!(store.token.borrow().as_deref(), Some("new-token"));
        assert_eq!(store.locks.get(), 1);
    }

    #[test]
    fn cached_401_grants_once_and_retries_once() {
        let store = MemoryStore::new(Some("stale-token"));
        let grants = Cell::new(0);
        let gets = Cell::new(0);
        let body = run_cached(
            &store,
            || {
                grants.set(grants.get() + 1);
                Ok("replacement".into())
            },
            |token| {
                gets.set(gets.get() + 1);
                match token {
                    "stale-token" => Err(http::GetError::Unauthorized),
                    "replacement" => Ok("response".into()),
                    _ => panic!("unexpected token"),
                }
            },
        )
        .expect("recovered GET");
        assert_eq!(body, "response");
        assert_eq!(grants.get(), 1);
        assert_eq!(gets.get(), 2);
        assert_eq!(store.token.borrow().as_deref(), Some("replacement"));
        assert_eq!(store.locks.get(), 1);
    }

    #[test]
    fn freshly_granted_401_does_not_grant_again() {
        let store = MemoryStore::new(None);
        let grants = Cell::new(0);
        let gets = Cell::new(0);
        let result = run_cached(
            &store,
            || {
                grants.set(grants.get() + 1);
                Ok("fresh-token".into())
            },
            |_| {
                gets.set(gets.get() + 1);
                Err(http::GetError::Unauthorized)
            },
        );
        assert_eq!(result, Err("VUT API authentication rejected"));
        assert_eq!(grants.get(), 1);
        assert_eq!(gets.get(), 1);
    }

    #[test]
    fn repeated_401_stops_after_one_replacement() {
        let store = MemoryStore::new(Some("stale-token"));
        let grants = Cell::new(0);
        let gets = Cell::new(0);
        let result = run_cached(
            &store,
            || {
                grants.set(grants.get() + 1);
                Ok("replacement".into())
            },
            |_| {
                gets.set(gets.get() + 1);
                Err(http::GetError::Unauthorized)
            },
        );
        assert_eq!(result, Err("VUT API authentication rejected"));
        assert_eq!(grants.get(), 1);
        assert_eq!(gets.get(), 2);
    }

    #[test]
    fn another_process_replacement_is_reused_without_grant() {
        let store = MemoryStore::new(Some("stale-token"));
        let result = run_cached(
            &store,
            || panic!("must not grant"),
            |token| match token {
                "stale-token" => {
                    *store.token.borrow_mut() = Some("other-process-token".into());
                    Err(http::GetError::Unauthorized)
                }
                "other-process-token" => Ok("response".into()),
                _ => panic!("unexpected token"),
            },
        );
        assert_eq!(result, Ok("response".into()));
    }

    #[test]
    fn cached_non_401_errors_do_not_grant() {
        for error in [
            http::GetError::Other("VUT API access denied"),
            http::GetError::Other("VUT API rate limited"),
            http::GetError::Other("unable to contact VUT API"),
        ] {
            let store = MemoryStore::new(Some("cached"));
            let expected = error.message();
            let result = run_cached(&store, || panic!("must not grant"), |_| Err(error));
            assert_eq!(result, Err(expected));
        }
    }

    #[test]
    fn store_errors_do_not_fall_back_to_grant_or_get() {
        let mut store = MemoryStore::new(Some("cached"));
        store.fail_load = true;
        assert_eq!(
            run_cached(&store, || panic!("grant"), |_| panic!("GET")),
            Err("store read failed")
        );

        let mut store = MemoryStore::new(None);
        store.fail_save = true;
        assert_eq!(
            run_cached(&store, || Ok("new".into()), |_| panic!("GET")),
            Err("store write failed")
        );
    }

    #[test]
    fn lock_errors_stop_miss_and_cached_401_before_grant_or_retry() {
        let mut missed = MemoryStore::new(None);
        missed.fail_lock = true;
        assert_eq!(
            run_cached(&missed, || panic!("grant"), |_| panic!("GET")),
            Err("store lock failed")
        );

        let mut cached = MemoryStore::new(Some("stale"));
        cached.fail_lock = true;
        let gets = Cell::new(0);
        assert_eq!(
            run_cached(
                &cached,
                || panic!("grant"),
                |_| {
                    gets.set(gets.get() + 1);
                    Err(http::GetError::Unauthorized)
                }
            ),
            Err("store lock failed")
        );
        assert_eq!(gets.get(), 1);
    }

    #[test]
    fn credentials_require_both_nonempty_fields() {
        for (uid, secret) in [
            (None, None),
            (Some("uid"), None),
            (None, Some("secret")),
            (Some(""), Some("secret")),
            (Some("uid"), Some("")),
        ] {
            assert!(
                validate_credentials(uid.map(str::to_owned), secret.map(str::to_owned)).is_err()
            );
        }
        let credentials = validate_credentials(Some("uid".into()), Some("secret".into()))
            .expect("complete credentials");
        assert_eq!(credentials.uid, "uid");
        assert_eq!(credentials.secret, "secret");
    }

    #[test]
    fn token_parser_requires_nonempty_bearer_token() {
        assert_eq!(
            parse_access_token(
                r#"{"access_token":"dummy-token","token_type":"Bearer","expires_in":3600}"#
            )
            .expect("valid bearer token"),
            "dummy-token"
        );
        for body in [
            "{}",
            r#"{"access_token":"","token_type":"Bearer"}"#,
            r#"{"access_token":"dummy-token","token_type":"Other"}"#,
            "not json",
        ] {
            assert!(parse_access_token(body).is_err());
        }
    }

    #[test]
    fn token_request_uses_fixed_endpoint_and_form_grant() {
        let client = Client::new();
        let credentials = Credentials {
            uid: "dummy-uid".into(),
            secret: "dummy-secret".into(),
        };
        let request = token_request(&client, &credentials).expect("build token request");

        assert_eq!(request.method(), reqwest::Method::POST);
        assert_eq!(request.url().as_str(), TOKEN_URL);
        assert_eq!(
            request.headers()[CONTENT_TYPE],
            "application/x-www-form-urlencoded"
        );
        assert_eq!(
            request.headers()[AUTHORIZATION],
            "Basic ZHVtbXktdWlkOmR1bW15LXNlY3JldA=="
        );
        assert_eq!(
            request.body().and_then(reqwest::blocking::Body::as_bytes),
            Some(b"grant_type=client_credentials".as_slice())
        );
    }

    #[test]
    fn token_status_distinguishes_rate_limit_from_auth_failure() {
        assert!(token_status(reqwest::StatusCode::OK).is_ok());
        assert_eq!(
            token_status(reqwest::StatusCode::TOO_MANY_REQUESTS),
            Err("VUT authentication rate limited")
        );
        assert_eq!(
            token_status(reqwest::StatusCode::UNAUTHORIZED),
            Err("VUT authentication failed")
        );
    }

    #[test]
    fn supplied_access_token_takes_precedence_over_client_credentials() {
        let source = select_source(
            Some("dummy-token".into()),
            Some("uid".into()),
            Some("secret".into()),
        )
        .expect("token source");
        assert!(matches!(source, AuthSource::Token(token) if token == "dummy-token"));
        assert_eq!(
            select_source(
                Some(String::new()),
                Some("uid".into()),
                Some("secret".into())
            )
            .err(),
            Some("VUT API access token is empty")
        );
        let source =
            select_source(None, Some("uid".into()), Some("secret".into())).expect("client source");
        assert!(matches!(source, AuthSource::ClientCredentials(_)));
    }

    #[cfg(unix)]
    struct TestDirectory(PathBuf);

    #[cfg(unix)]
    impl TestDirectory {
        fn new(label: &str) -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock after epoch")
                .as_nanos();
            Self(std::env::temp_dir().join(format!(
                "studis-auth-{label}-{}-{nonce}",
                std::process::id()
            )))
        }
    }

    #[cfg(unix)]
    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[cfg(unix)]
    fn spawn_coordination_helper(directory: &Path, action: &str, id: &str) -> Child {
        Command::new(std::env::current_exe().expect("current test executable"))
            .args([
                "--ignored",
                "--exact",
                "auth::tests::coordinated_process_helper",
            ])
            .env("STUDIS_AUTH_TEST_DIRECTORY", directory)
            .env("STUDIS_AUTH_TEST_ACTION", action)
            .env("STUDIS_AUTH_TEST_ID", id)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn auth helper")
    }

    #[cfg(unix)]
    fn wait_for_files(paths: &[PathBuf]) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while paths.iter().any(|path| !path.exists()) {
            assert!(Instant::now() < deadline, "timed out waiting for helpers");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[cfg(unix)]
    fn finish_helpers(children: [Child; 2]) {
        for child in children {
            let output = child.wait_with_output().expect("wait for auth helper");
            assert!(
                output.status.success(),
                "helper failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }

    #[cfg(unix)]
    fn grant_count(directory: &Path) -> usize {
        fs::read_to_string(directory.join("grants"))
            .expect("grant counter")
            .lines()
            .count()
    }

    #[cfg(unix)]
    #[test]
    fn simultaneous_cache_misses_issue_one_grant_across_processes() {
        let directory = TestDirectory::new("simultaneous-miss");
        let children = [
            spawn_coordination_helper(&directory.0, "miss", "one"),
            spawn_coordination_helper(&directory.0, "miss", "two"),
        ];
        wait_for_files(&[directory.0.join("ready-one"), directory.0.join("ready-two")]);
        fs::write(directory.0.join("go"), b"").expect("release helpers");
        finish_helpers(children);
        assert_eq!(grant_count(&directory.0), 1);
    }

    #[cfg(unix)]
    #[test]
    fn simultaneous_cached_401s_issue_one_replacement_across_processes() {
        let directory = TestDirectory::new("simultaneous-401");
        let store = PlatformTokenStore::new_in(&directory.0, "synthetic-uid")
            .expect("create initial store");
        store.save("stale-token").expect("save stale token");

        let children = [
            spawn_coordination_helper(&directory.0, "401", "one"),
            spawn_coordination_helper(&directory.0, "401", "two"),
        ];
        wait_for_files(&[directory.0.join("stale-one"), directory.0.join("stale-two")]);
        fs::write(directory.0.join("go"), b"").expect("release helpers");
        finish_helpers(children);
        assert_eq!(grant_count(&directory.0), 1);
    }

    #[cfg(unix)]
    #[test]
    #[ignore = "child-process helper"]
    fn coordinated_process_helper() {
        let directory = PathBuf::from(
            std::env::var_os("STUDIS_AUTH_TEST_DIRECTORY").expect("helper directory"),
        );
        let action = std::env::var("STUDIS_AUTH_TEST_ACTION").expect("helper action");
        let id = std::env::var("STUDIS_AUTH_TEST_ID").expect("helper id");
        let store = PlatformTokenStore::new_in(&directory, "synthetic-uid").expect("helper store");

        if action == "miss" {
            fs::write(directory.join(format!("ready-{id}")), b"").expect("signal ready");
            wait_for_files(&[directory.join("go")]);
        }

        let result = run_cached(
            &store,
            || {
                let mut grants = OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(directory.join("grants"))
                    .expect("open grant counter");
                grants.write_all(b"grant\n").expect("record grant");
                std::thread::sleep(Duration::from_millis(200));
                Ok("replacement-token".to_owned())
            },
            |token| match (action.as_str(), token) {
                ("miss", "replacement-token") | ("401", "replacement-token") => {
                    Ok("response".to_owned())
                }
                ("401", "stale-token") => {
                    fs::write(directory.join(format!("stale-{id}")), b"")
                        .expect("signal stale GET");
                    wait_for_files(&[directory.join("go")]);
                    Err(http::GetError::Unauthorized)
                }
                _ => panic!("unexpected helper token"),
            },
        );
        assert_eq!(result.as_deref(), Ok("response"));
    }
}
