//! Read-only account context operations.

use reqwest::blocking::{Client, Request};

use crate::{auth, resources};

const ROLES_URL: &str = "https://api.vut.cz/api/moje/info/role/v1";

pub(crate) fn roles_request(client: &Client, token: &str) -> Result<Request, &'static str> {
    client
        .get(ROLES_URL)
        .bearer_auth(token)
        .build()
        .map_err(|_| "unable to prepare VUT account roles request")
}

pub(crate) fn parse_roles(body: &str) -> Result<serde_json::Value, &'static str> {
    resources::parse_required_array(body, "role", "invalid VUT account roles response")
}

pub(crate) fn fetch_roles() -> Result<serde_json::Value, &'static str> {
    parse_roles(&auth::get_body(roles_request)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::{blocking::Client, header::AUTHORIZATION};
    use serde_json::json;

    #[test]
    fn roles_request_is_fixed_bearer_get() {
        let request = roles_request(&Client::new(), "dummy-token").expect("build roles request");
        assert_eq!(request.method(), reqwest::Method::GET);
        assert_eq!(
            request.url().as_str(),
            "https://api.vut.cz/api/moje/info/role/v1"
        );
        assert!(request.url().query().is_none());
        assert_eq!(request.headers()[AUTHORIZATION], "Bearer dummy-token");
        assert!(request.body().is_none());
    }

    #[test]
    fn roles_parser_requires_role_array_and_preserves_raw_response() {
        let raw = json!({
            "format": "json",
            "data": {"role": [{"future": {"nested": true}}]},
            "extra": "untouched"
        });
        assert_eq!(parse_roles(&raw.to_string()).expect("account roles"), raw);
        assert_eq!(
            parse_roles(r#"{"data":{"role":[]}}"#).expect("empty roles"),
            json!({"data":{"role":[]}})
        );

        for body in [
            "not json",
            "{}",
            r#"{"data":{}}"#,
            r#"{"data":{"role":{}}}"#,
        ] {
            assert!(parse_roles(body).is_err(), "accepted {body}");
        }
    }
}
