//! Bearer-token sourcing for read-only requests.

use reqwest::blocking::{Client, Request};
use reqwest::header::CONTENT_TYPE;

use crate::http;

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

pub(crate) fn session() -> Result<(Client, String), &'static str> {
    let source = source_from_env()?;
    let client = http::client()?;
    let credentials = match source {
        AuthSource::Token(token) => return Ok((client, token)),
        AuthSource::ClientCredentials(credentials) => credentials,
    };
    let request = token_request(&client, &credentials)?;
    let response = client
        .execute(request)
        .map_err(|_| "unable to contact VUT authentication service")?;
    token_status(response.status())?;
    let body = response
        .text()
        .map_err(|_| "unable to read VUT authentication response")?;
    Ok((client, parse_access_token(&body)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::AUTHORIZATION;

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
    fn supplied_access_token_takes_precedence_without_client_credentials() {
        let source = select_source(Some("dummy-token".into()), None, None).expect("token source");
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
