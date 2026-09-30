//! Read-only personal teaching and teaching weeks.

use reqwest::blocking::{Client, Request};

use crate::{auth, resources};

pub(crate) const TEACHING_URL: &str = "https://api.vut.cz/api/rozvrh/osobni/vyucovani/v4";
pub(crate) const WEEKS_URL: &str = "https://api.vut.cz/api/rozvrh/osobni/vyucovani/tydny/v2";
pub(crate) const TERMS_URL: &str = "https://api.vut.cz/api/rozvrh/osobni/terminy/v3";

pub(crate) fn teaching_request(
    client: &Client,
    token: &str,
    from: &str,
    to: &str,
) -> Result<Request, &'static str> {
    client
        .get(TEACHING_URL)
        .bearer_auth(token)
        .query(&[("datum_od", from), ("datum_do", to)])
        .build()
        .map_err(|_| "unable to prepare VUT teaching request")
}

pub(crate) fn weeks_request(
    client: &Client,
    token: &str,
    from: &str,
    to: &str,
) -> Result<Request, &'static str> {
    client
        .get(WEEKS_URL)
        .bearer_auth(token)
        .query(&[("datum_od", from), ("datum_do", to)])
        .build()
        .map_err(|_| "unable to prepare VUT teaching weeks request")
}

pub(crate) fn terms_request(client: &Client, token: &str) -> Result<Request, &'static str> {
    client
        .get(TERMS_URL)
        .bearer_auth(token)
        .build()
        .map_err(|_| "unable to prepare VUT terms request")
}

pub(crate) fn parse_teaching(body: &str) -> Result<serde_json::Value, &'static str> {
    resources::parse_list(body, "vyucovani")
}

pub(crate) fn parse_weeks(body: &str) -> Result<serde_json::Value, &'static str> {
    resources::parse_list(body, "tydny")
}

pub(crate) fn parse_terms(body: &str) -> Result<serde_json::Value, &'static str> {
    resources::parse_required_array(body, "terminy", "invalid VUT terms response")
}

pub(crate) fn fetch_teaching(from: &str, to: &str) -> Result<serde_json::Value, &'static str> {
    parse_teaching(&auth::get_body(|client, token| {
        teaching_request(client, token, from, to)
    })?)
}

pub(crate) fn fetch_weeks(from: &str, to: &str) -> Result<serde_json::Value, &'static str> {
    parse_weeks(&auth::get_body(|client, token| {
        weeks_request(client, token, from, to)
    })?)
}

pub(crate) fn fetch_terms() -> Result<serde_json::Value, &'static str> {
    parse_terms(&auth::get_body(terms_request)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::AUTHORIZATION;
    use serde_json::json;

    type Parser = fn(&str) -> Result<serde_json::Value, &'static str>;

    #[test]
    fn requests_use_fixed_gets_and_both_range_parameters() {
        let client = Client::new();
        for (request, url, from, to) in [
            (
                teaching_request(&client, "dummy", "2026-09-29T08:00", "2026-09-29T18:00"),
                TEACHING_URL,
                "2026-09-29T08:00",
                "2026-09-29T18:00",
            ),
            (
                weeks_request(&client, "dummy", "2026-09-28", "2026-10-04"),
                WEEKS_URL,
                "2026-09-28",
                "2026-10-04",
            ),
        ] {
            let request = request.expect("schedule request");
            assert_eq!(request.method(), reqwest::Method::GET);
            assert_eq!(request.url().as_str().split('?').next(), Some(url));
            assert_eq!(
                request.url().query_pairs().collect::<Vec<_>>(),
                vec![
                    ("datum_od".into(), from.into()),
                    ("datum_do".into(), to.into())
                ]
            );
            assert_eq!(request.headers()[AUTHORIZATION], "Bearer dummy");
            assert!(request.body().is_none());
        }
    }

    #[test]
    fn parsers_preserve_raw_data_and_empty_upstream_envelope() {
        let parsers: [(Parser, &str); 2] = [(parse_teaching, "vyucovani"), (parse_weeks, "tydny")];
        for (parse, key) in parsers {
            let raw = json!({"format":"json","data":{(key):[{"unknown":1}]},"rows_count":1});
            assert_eq!(parse(&raw.to_string()).expect("schedule data"), raw);
            assert_eq!(
                parse(r#"{"data":{}}"#).expect("empty schedule"),
                json!({"data":{}})
            );
        }
    }

    #[test]
    fn parsers_reject_bad_or_wrong_resource_envelopes() {
        let parsers: [(Parser, &str); 2] = [(parse_teaching, "tydny"), (parse_weeks, "vyucovani")];
        for (parse, wrong) in parsers {
            for body in ["not json", "{}", r#"{"data":null}"#] {
                assert!(parse(body).is_err());
            }
            let raw = json!({"data":{(wrong):[{}]}});
            assert!(parse(&raw.to_string()).is_err());
            let expected = if wrong == "tydny" {
                "vyucovani"
            } else {
                "tydny"
            };
            let non_array = json!({"data":{(expected):{}}});
            assert!(parse(&non_array.to_string()).is_err());
        }
    }

    #[test]
    fn terms_request_is_fixed_bearer_get_without_query_or_body() {
        let request = terms_request(&Client::new(), "dummy").expect("terms request");
        assert_eq!(request.method(), reqwest::Method::GET);
        assert_eq!(
            request.url().as_str(),
            "https://api.vut.cz/api/rozvrh/osobni/terminy/v3"
        );
        assert!(request.url().query().is_none());
        assert_eq!(request.headers()[AUTHORIZATION], "Bearer dummy");
        assert!(request.body().is_none());
    }

    #[test]
    fn terms_parser_requires_terminy_array_and_preserves_raw_response() {
        let raw = json!({
            "format": "json",
            "data": {"terminy": [{"future": true}]},
            "extra": "untouched"
        });
        assert_eq!(parse_terms(&raw.to_string()).expect("terms"), raw);
        assert_eq!(
            parse_terms(r#"{"data":{"terminy":[]}}"#).expect("empty terms"),
            json!({"data":{"terminy":[]}})
        );
        for body in [
            "not json",
            "{}",
            r#"{"data":{}}"#,
            r#"{"data":{"terminy":{}}}"#,
        ] {
            assert!(parse_terms(body).is_err(), "accepted {body}");
        }
    }
}
