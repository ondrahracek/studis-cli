//! OAuth client credentials for one read-only request.

use reqwest::blocking::{Client, Request};
use reqwest::header::CONTENT_TYPE;

pub(crate) const TOKEN_URL: &str = "https://id.vut.cz/auth/common/oauth2/token";

pub(crate) struct Credentials {
    pub(crate) uid: String,
    pub(crate) secret: String,
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

pub(crate) fn credentials_from_env() -> Result<Credentials, &'static str> {
    validate_credentials(
        std::env::var("VUT_API_CLIENT_UID").ok(),
        std::env::var("VUT_API_CLIENT_SECRET").ok(),
    )
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
}
