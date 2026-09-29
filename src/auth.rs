//! Bearer-token sourcing for read-only requests.

use reqwest::blocking::{Client, Request};
use reqwest::header::CONTENT_TYPE;

use crate::{
    http,
    token_store::{KeyringStore, TokenStore},
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
    let cached = store.load()?;
    let token = match cached {
        Some(ref token) => token.clone(),
        None => {
            let token = issue()?;
            store.save(&token)?;
            token
        }
    };

    match get(&token) {
        Ok(body) => Ok(body),
        Err(http::GetError::Unauthorized) if cached.is_some() => {
            let replacement = match store.load()? {
                Some(newer) if newer != token => newer,
                _ => {
                    let newer = issue()?;
                    store.save(&newer)?;
                    newer
                }
            };
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
            let store = KeyringStore::new(&credentials.uid)?;
            run_cached(&store, || issue_token(&client, &credentials), get)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::AUTHORIZATION;
    use std::cell::{Cell, RefCell};

    struct MemoryStore {
        token: RefCell<Option<String>>,
        fail_load: bool,
        fail_save: bool,
    }

    impl MemoryStore {
        fn new(token: Option<&str>) -> Self {
            Self {
                token: RefCell::new(token.map(str::to_owned)),
                fail_load: false,
                fail_save: false,
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
}
