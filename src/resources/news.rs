//! Read-only study news.

use reqwest::blocking::{Client, Request};

use crate::{auth, resources};

pub(crate) const NEWS_URL: &str = "https://api.vut.cz/api/moje/studia/aktuality/v1";

pub(crate) fn request(client: &Client, token: &str, since: &str) -> Result<Request, &'static str> {
    client
        .get(NEWS_URL)
        .bearer_auth(token)
        .query(&[("datum_od", since)])
        .build()
        .map_err(|_| "unable to prepare VUT news request")
}

pub(crate) fn parse(body: &str) -> Result<serde_json::Value, &'static str> {
    resources::parse_list(body, "dokumenty")
}

pub(crate) fn fetch(since: &str) -> Result<serde_json::Value, &'static str> {
    parse(&auth::get_body(|client, token| {
        request(client, token, since)
    })?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::AUTHORIZATION;
    use serde_json::json;

    #[test]
    fn request_uses_fixed_get_and_since_parameter() {
        let request = request(&Client::new(), "dummy-token", "2026-09-29").expect("news request");
        assert_eq!(request.method(), reqwest::Method::GET);
        assert_eq!(request.url().as_str().split('?').next(), Some(NEWS_URL));
        assert_eq!(
            request.url().query_pairs().collect::<Vec<_>>(),
            vec![("datum_od".into(), "2026-09-29".into())]
        );
        assert_eq!(request.headers()[AUTHORIZATION], "Bearer dummy-token");
        assert!(request.body().is_none());
    }

    #[test]
    fn parser_preserves_raw_news_and_accepts_upstream_empty_object() {
        let raw = json!({"format":"json","data":{"dokumenty":[{"future_field":true}]},"extra":7});
        assert_eq!(parse(&raw.to_string()).expect("news"), raw);
        assert_eq!(
            parse(r#"{"format":"json","data":{}}"#).expect("empty news"),
            json!({"format":"json","data":{}})
        );
    }

    #[test]
    fn parser_rejects_invalid_news_envelope() {
        for body in [
            "not json",
            "{}",
            r#"{"data":null}"#,
            r#"{"data":{"dokumenty":{}}}"#,
        ] {
            assert!(parse(body).is_err(), "accepted {body}");
        }
    }
}
