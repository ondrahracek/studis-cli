//! Read-only study operations.

use reqwest::blocking::{Client, Request};

use crate::{auth, resources};

pub(crate) const STUDIES_URL: &str = "https://api.vut.cz/api/moje/studia/v1";
const STUDY_INDEX_URL_PREFIX: &str = "https://api.vut.cz/api/moje/studia/studium";

pub(crate) fn studies_request(client: &Client, token: &str) -> Result<Request, &'static str> {
    client
        .get(STUDIES_URL)
        .bearer_auth(token)
        .build()
        .map_err(|_| "unable to prepare VUT studies request")
}

pub(crate) fn parse_studies(body: &str) -> Result<serde_json::Value, &'static str> {
    resources::parse_required_array(body, "studia", "invalid VUT studies response")
}

pub(crate) fn fetch() -> Result<serde_json::Value, &'static str> {
    parse_studies(&auth::get_body(studies_request)?)
}

pub(crate) fn index_request(
    client: &Client,
    token: &str,
    study_id: u64,
) -> Result<Request, &'static str> {
    client
        .get(format!("{STUDY_INDEX_URL_PREFIX}/{study_id}/index/v1"))
        .bearer_auth(token)
        .build()
        .map_err(|_| "unable to prepare VUT study index request")
}

pub(crate) fn parse_index(body: &str) -> Result<serde_json::Value, &'static str> {
    let raw: serde_json::Value =
        serde_json::from_str(body).map_err(|_| "invalid VUT study index response")?;
    let data = raw
        .get("data")
        .and_then(serde_json::Value::as_object)
        .ok_or("invalid VUT study index response")?;
    if !data.is_empty() && !data.get("studia").is_some_and(serde_json::Value::is_array) {
        return Err("invalid VUT study index response");
    }
    Ok(raw)
}

pub(crate) fn fetch_index(study_id: u64) -> Result<serde_json::Value, &'static str> {
    parse_index(&auth::get_body(|client, token| {
        index_request(client, token, study_id)
    })?)
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

    #[test]
    fn index_request_is_fixed_bearer_get_for_explicit_study() {
        let request = index_request(&Client::new(), "dummy-token", 7).expect("build index request");
        assert_eq!(request.method(), reqwest::Method::GET);
        assert_eq!(
            request.url().as_str(),
            "https://api.vut.cz/api/moje/studia/studium/7/index/v1"
        );
        assert!(request.url().query().is_none());
        assert_eq!(request.headers()[AUTHORIZATION], "Bearer dummy-token");
        assert!(request.body().is_none());
    }

    #[test]
    fn index_parser_accepts_observed_empty_object_and_preserves_raw_response() {
        let raw = json!({
            "format": "json",
            "data": {"studia": [{"studium_id": 7, "index": 42, "future": true}]},
            "extra": "untouched"
        });
        assert_eq!(parse_index(&raw.to_string()).expect("study index"), raw);
        assert_eq!(
            parse_index(r#"{"data":{"studia":[]}}"#).expect("empty study index"),
            json!({"data":{"studia":[]}})
        );
        assert_eq!(
            parse_index(r#"{"format":"json","data":{}}"#).expect("empty index object"),
            json!({"format":"json","data":{}})
        );

        for body in [
            "not json",
            "{}",
            r#"{"data":null}"#,
            r#"{"data":{"studia":{}}}"#,
            r#"{"data":{"unexpected":[]}}"#,
        ] {
            assert!(parse_index(body).is_err(), "accepted {body}");
        }
    }
}
