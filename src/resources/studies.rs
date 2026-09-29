//! Read-only study operations.

use reqwest::blocking::{Client, Request};

use crate::{auth, http};

pub(crate) const STUDIES_URL: &str = "https://api.vut.cz/api/moje/studia/v1";

pub(crate) fn studies_request(client: &Client, token: &str) -> Result<Request, &'static str> {
    client
        .get(STUDIES_URL)
        .bearer_auth(token)
        .build()
        .map_err(|_| "unable to prepare VUT studies request")
}

pub(crate) fn parse_studies(body: &str) -> Result<serde_json::Value, &'static str> {
    let raw: serde_json::Value =
        serde_json::from_str(body).map_err(|_| "invalid VUT studies response")?;
    if !raw
        .get("data")
        .and_then(|data| data.get("studia"))
        .is_some_and(serde_json::Value::is_array)
    {
        return Err("invalid VUT studies response");
    }
    Ok(raw)
}

pub(crate) fn fetch() -> Result<serde_json::Value, &'static str> {
    let credentials = auth::credentials_from_env()?;
    let client = http::client()?;

    let token_request = auth::token_request(&client, &credentials)?;
    let token_response = client
        .execute(token_request)
        .map_err(|_| "unable to contact VUT authentication service")?;
    if token_response.status() != reqwest::StatusCode::OK {
        return Err("VUT authentication failed");
    }
    let token_body = token_response
        .text()
        .map_err(|_| "unable to read VUT authentication response")?;
    let token = auth::parse_access_token(&token_body)?;

    let studies_request = studies_request(&client, &token)?;
    let studies_response = client
        .execute(studies_request)
        .map_err(|_| "unable to contact VUT studies service")?;
    if studies_response.status() != reqwest::StatusCode::OK {
        return Err("VUT studies request failed");
    }
    let studies_body = studies_response
        .text()
        .map_err(|_| "unable to read VUT studies response")?;
    parse_studies(&studies_body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::AUTHORIZATION;
    use serde_json::json;

    #[test]
    fn parser_accepts_empty_array_and_preserves_unknown_fields() {
        let body = r#"{"format":"json","data":{"studia":[{"studium_id":7,"unknown":{"nested":true}}]},"extra":"untouched"}"#;
        let raw = parse_studies(body).expect("valid studies");
        assert_eq!(
            raw,
            json!({
                "format":"json",
                "data":{"studia":[{"studium_id":7,"unknown":{"nested":true}}]},
                "extra":"untouched"
            })
        );
        assert_eq!(
            parse_studies(r#"{"data":{"studia":[]}}"#).expect("empty studies"),
            json!({"data":{"studia":[]}})
        );
    }

    #[test]
    fn parser_rejects_missing_or_invalid_studies_array() {
        for body in [
            "{}",
            r#"{"data":{}}"#,
            r#"{"data":{"studia":{}}}"#,
            "not json",
        ] {
            assert!(parse_studies(body).is_err());
        }
    }

    #[test]
    fn studies_request_is_fixed_bearer_get() {
        let request =
            studies_request(&Client::new(), "dummy-token").expect("build studies request");
        assert_eq!(request.method(), reqwest::Method::GET);
        assert_eq!(request.url().as_str(), STUDIES_URL);
        assert_eq!(request.headers()[AUTHORIZATION], "Bearer dummy-token");
        assert!(request.body().is_none());
    }
}
