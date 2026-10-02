//! Documented read-only subject endpoints.

use reqwest::blocking::{Client, Request};
use serde_json::Value;
use std::collections::BTreeSet;

use crate::{auth, moodle_url, resources};

const SUBJECT_URL: &str = "https://api.vut.cz/api/predmety/aktualni_predmet";
const TIMETABLE_URL: &str = "https://api.vut.cz/api/rozvrh/aktualni_predmet";

pub(crate) fn catalog_request(
    client: &Client,
    token: &str,
    offering_id: u64,
) -> Result<Request, &'static str> {
    client
        .get(format!("{SUBJECT_URL}/{offering_id}/v1"))
        .bearer_auth(token)
        .build()
        .map_err(|_| "unable to prepare VUT subject request")
}

pub(crate) fn moodle_request(
    client: &Client,
    token: &str,
    offering_id: u64,
) -> Result<Request, &'static str> {
    client
        .get(format!("{SUBJECT_URL}/{offering_id}/odkazy/moodle/v1"))
        .bearer_auth(token)
        .build()
        .map_err(|_| "unable to prepare VUT Moodle link request")
}

pub(crate) fn timetable_request(
    client: &Client,
    token: &str,
    offering_id: u64,
    from: &str,
    to: &str,
) -> Result<Request, &'static str> {
    let from_date = from.get(..10).ok_or("invalid course timetable window")?;
    let to_date = to.get(..10).ok_or("invalid course timetable window")?;
    client
        .get(format!("{TIMETABLE_URL}/{offering_id}/vyucovani/v1"))
        .bearer_auth(token)
        .query(&[("datum_od", from_date), ("datum_do", to_date)])
        .build()
        .map_err(|_| "unable to prepare VUT course timetable request")
}

pub(crate) fn parse_catalog(body: &str) -> Result<serde_json::Value, &'static str> {
    resources::parse_required_array(body, "predmety", "invalid VUT subject response")
}

pub(crate) fn parse_moodle(body: &str) -> Result<serde_json::Value, &'static str> {
    resources::parse_list(body, "odkazy")
}

pub(crate) fn parse_timetable(body: &str) -> Result<serde_json::Value, &'static str> {
    resources::parse_list(body, "vyucovani")
}

pub(crate) fn fetch_catalog(offering_id: u64) -> Result<serde_json::Value, &'static str> {
    parse_catalog(&auth::get_body(|client, token| {
        catalog_request(client, token, offering_id)
    })?)
}

pub(crate) fn fetch_moodle(offering_id: u64) -> Result<serde_json::Value, &'static str> {
    parse_moodle(&auth::get_body(|client, token| {
        moodle_request(client, token, offering_id)
    })?)
}

pub(crate) fn fetch_timetable(
    offering_id: u64,
    from: &str,
    to: &str,
) -> Result<serde_json::Value, &'static str> {
    parse_timetable(&auth::get_body(|client, token| {
        timetable_request(client, token, offering_id, from, to)
    })?)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum MoodleCourseLink {
    Missing,
    Unique(String),
    Ambiguous,
}

pub(crate) fn verified_moodle_course_url(raw: &Value) -> MoodleCourseLink {
    let mut candidates = raw
        .pointer("/data/odkazy")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|link| link.get("odkaz_moodle").and_then(Value::as_str))
        .filter_map(moodle_url::canonical_course_url)
        .collect::<BTreeSet<_>>();
    match candidates.len() {
        0 => MoodleCourseLink::Missing,
        1 => MoodleCourseLink::Unique(candidates.pop_first().expect("one Moodle course URL")),
        _ => MoodleCourseLink::Ambiguous,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::AUTHORIZATION;
    use serde_json::json;

    #[test]
    fn subject_requests_are_fixed_bearer_gets() {
        let client = Client::new();
        let requests = [
            (
                catalog_request(&client, "dummy", 305747).unwrap(),
                "https://api.vut.cz/api/predmety/aktualni_predmet/305747/v1",
            ),
            (
                moodle_request(&client, "dummy", 305747).unwrap(),
                "https://api.vut.cz/api/predmety/aktualni_predmet/305747/odkazy/moodle/v1",
            ),
        ];
        for (request, url) in requests {
            assert_eq!(request.method(), reqwest::Method::GET);
            assert_eq!(request.url().as_str(), url);
            assert_eq!(request.headers()[AUTHORIZATION], "Bearer dummy");
            assert!(request.body().is_none());
        }
        let timetable = timetable_request(
            &client,
            "dummy",
            305747,
            "2026-09-28T00:00",
            "2026-10-05T23:59",
        )
        .unwrap();
        assert_eq!(timetable.method(), reqwest::Method::GET);
        assert_eq!(
            timetable.url().path(),
            "/api/rozvrh/aktualni_predmet/305747/vyucovani/v1"
        );
        assert_eq!(
            timetable.url().query_pairs().collect::<Vec<_>>(),
            vec![
                ("datum_od".into(), "2026-09-28".into()),
                ("datum_do".into(), "2026-10-05".into())
            ]
        );
        assert_eq!(timetable.headers()[AUTHORIZATION], "Bearer dummy");
        assert!(timetable.body().is_none());
    }

    #[test]
    fn subject_parsers_preserve_raw_and_reject_malformed_data() {
        let catalog = json!({"data":{"predmety":[{"predmet_id":1,"aktualni_predmety":[{"aktualni_predmet_id":2,"future":true}]}]}});
        assert_eq!(parse_catalog(&catalog.to_string()), Ok(catalog));
        assert!(parse_catalog(r#"{"data":{}}"#).is_err());
        assert_eq!(parse_moodle(r#"{"data":{}}"#), Ok(json!({"data":{}})));
        assert_eq!(parse_timetable(r#"{"data":{}}"#), Ok(json!({"data":{}})));
        assert!(parse_moodle(r#"{"data":{"odkazy":{}}}"#).is_err());
        assert!(parse_timetable(r#"{"data":{"vyucovani":{}}}"#).is_err());
    }

    #[test]
    fn verified_moodle_link_accepts_only_the_fixed_course_route() {
        let raw = json!({"data":{"odkazy":[
            {"odkaz_moodle":"https://moodle.vut.cz.evil.example/course/view.php?id=42"},
            {"odkaz_moodle":"https://moodle.vut.cz/mod/resource/view.php?id=5"},
            {"odkaz_moodle":"https://moodle.vut.cz/course/view.php?id=42"}
        ]}});
        assert_eq!(
            verified_moodle_course_url(&raw),
            MoodleCourseLink::Unique("https://moodle.vut.cz/course/view.php?id=42".into())
        );
        assert_eq!(
            verified_moodle_course_url(&json!({"data":{"odkazy":[]}})),
            MoodleCourseLink::Missing
        );
        assert_eq!(
            verified_moodle_course_url(&json!({"data":{"odkazy":[
                {"odkaz_moodle":"https://moodle.vut.cz/course/view.php?id=42"},
                {"odkaz_moodle":"https://moodle.vut.cz/course/view.php?id=43"}
            ]}})),
            MoodleCourseLink::Ambiguous
        );
        assert_eq!(
            verified_moodle_course_url(&json!({"data":{"odkazy":[
                {"odkaz_moodle":"https://moodle.vut.cz/course/view.php?id=00042"},
                {"odkaz_moodle":"https://moodle.vut.cz/course/view.php?id=42"}
            ]}})),
            MoodleCourseLink::Unique("https://moodle.vut.cz/course/view.php?id=42".into())
        );
        assert_eq!(
            moodle_url::canonical_course_url("https://moodle.vut.cz:444/course/view.php?id=42"),
            None
        );
        for unsafe_url in [
            "https://moodle.vut.cz/course/view.php?id=42&token=secret",
            "https://moodle.vut.cz/course/view.php?id=42&id=43",
            "https://moodle.vut.cz/course/view.php?id=42#private",
        ] {
            assert_eq!(
                moodle_url::canonical_course_url(unsafe_url),
                None,
                "accepted {unsafe_url}"
            );
        }
        assert_eq!(
            moodle_url::canonical_course_url("https://moodle.vut.cz/course/view.php?id=00042")
                .as_deref(),
            Some("https://moodle.vut.cz/course/view.php?id=42")
        );
    }
}
