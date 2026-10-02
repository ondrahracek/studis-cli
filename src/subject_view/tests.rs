use super::*;
use serde_json::json;

#[test]
fn subject_view_v1_serialization_contract() {
    let view = SubjectView {
        schema_version: 1,
        subject: SubjectIdentity {
            offering_id: 42,
            subject_id: 142,
            faculty_id: 13,
            academic_year: 2026,
            semester_type_id: 2,
            study_id: 7,
        },
        requested: RequestedScope {
            from: "2026-09-01T00:00".into(),
            to: "2027-08-31T23:59".into(),
            news_since: "1900-01-01".into(),
            max_news: 0,
        },
        sections: SubjectSections {
            catalog: Section::available(
                Source {
                    url: "https://example.invalid/catalog".into(),
                    fetched_at_unix_ms: 1,
                },
                json!({"records":[{"zkratka":"IZP"}]}),
            ),
            study_record: Section::available(
                Source {
                    url: "https://example.invalid/index".into(),
                    fetched_at_unix_ms: 2,
                },
                json!({"records":[{"studium_id":7}]}),
            )
            .limitation("assessment_table_may_require_web_access"),
            announcements: Section::empty(
                Source {
                    url: "https://example.invalid/news".into(),
                    fetched_at_unix_ms: 3,
                },
                json!({"items":[],"returned_count":0,"matching_count":0,"truncated":false,"complete":true}),
            ),
            personal_schedule: Section::empty(
                Source {
                    url: "https://example.invalid/schedule".into(),
                    fetched_at_unix_ms: 4,
                },
                json!({"entries":[]}),
            ),
            course_timetable: Section::unavailable(
                Some(Source {
                    url: "https://example.invalid/timetable".into(),
                    fetched_at_unix_ms: 5,
                }),
                "unverified_empty_response",
                Value::Null,
            ),
            moodle: Section::unavailable(None, "no_verified_moodle_link", Value::Null),
        },
        warnings: vec!["synthetic_warning"],
    };

    assert_eq!(
        serde_json::to_value(view).expect("serialize subject view"),
        json!({
            "schema_version":1,
            "subject":{
                "offering_id":42,
                "subject_id":142,
                "faculty_id":13,
                "academic_year":2026,
                "semester_type_id":2,
                "study_id":7
            },
            "requested":{
                "from":"2026-09-01T00:00",
                "to":"2027-08-31T23:59",
                "news_since":"1900-01-01",
                "max_news":0
            },
            "sections":{
                "catalog":{
                    "status":"available",
                    "sources":[{"url":"https://example.invalid/catalog","fetched_at_unix_ms":1}],
                    "data":{"records":[{"zkratka":"IZP"}]}
                },
                "study_record":{
                    "status":"available",
                    "sources":[{"url":"https://example.invalid/index","fetched_at_unix_ms":2}],
                    "data":{"records":[{"studium_id":7}]},
                    "limitations":["assessment_table_may_require_web_access"]
                },
                "announcements":{
                    "status":"empty",
                    "sources":[{"url":"https://example.invalid/news","fetched_at_unix_ms":3}],
                    "data":{"items":[],"returned_count":0,"matching_count":0,"truncated":false,"complete":true}
                },
                "personal_schedule":{
                    "status":"empty",
                    "sources":[{"url":"https://example.invalid/schedule","fetched_at_unix_ms":4}],
                    "data":{"entries":[]}
                },
                "course_timetable":{
                    "status":"unavailable",
                    "reason":"unverified_empty_response",
                    "sources":[{"url":"https://example.invalid/timetable","fetched_at_unix_ms":5}],
                    "data":null
                },
                "moodle":{
                    "status":"unavailable",
                    "reason":"no_verified_moodle_link",
                    "sources":[],
                    "data":null
                }
            },
            "warnings":["synthetic_warning"]
        })
    );
}

fn entry(
    offering_id: u64,
    code: &str,
    name: &str,
    start: &str,
    end: &str,
    el_index_id: u64,
) -> Value {
    json!({
        "aktualni_predmet_id":offering_id,
        "predmet_id":offering_id + 100,
        "fakulta_id":13,
        "akrok":2026,
        "typ_semestru_id":2,
        "el_index_id":el_index_id,
        "zkratka":code,
        "ap_nazev":name,
        "zacatek_semestru":start,
        "konec_semestru":end
    })
}

fn index(study_id: u64, entries: Vec<Value>) -> Value {
    json!({"data":{"studia":[{"studium_id":study_id,"index":entries}]}})
}

#[test]
fn explicit_identity_route_preserves_distinct_index_records() {
    let index = index(
        7,
        vec![
            entry(42, "IZP", "Programming", "2026-09-01", "2027-01-31", 1),
            entry(42, "IZP", "Programming", "2026-09-01", "2027-01-31", 2),
        ],
    );
    let resolved = resolve_lookup_in_indexes(&[(7, index)], None, Some(42), "2026-09-30")
        .expect("matching offering");
    assert_eq!(resolved.identity.offering_id, 42);
    assert_eq!(resolved.records.len(), 2);
}

#[test]
fn resolver_prefers_exact_code_then_name_case_insensitively() {
    let rows = vec![
        entry(42, "IZP", "Other", "2026-09-01", "2027-01-31", 1),
        entry(42, "LEGACY", "Other", "2026-09-01", "2027-01-31", 4),
        entry(43, "XXX", "izp", "2026-09-01", "2027-01-31", 2),
        entry(44, "YYY", "Algorithms", "2026-09-01", "2027-01-31", 3),
    ];
    let code = resolve_lookup_in_indexes(
        &[(7, index(7, rows.clone()))],
        Some("izp"),
        None,
        "2026-09-30",
    )
    .unwrap();
    assert_eq!(code.identity.offering_id, 42);
    assert_eq!(code.records.len(), 2);
    let name = resolve_lookup_in_indexes(
        &[(7, index(7, rows))],
        Some("algorithms"),
        None,
        "2026-09-30",
    )
    .unwrap();
    assert_eq!(name.identity.offering_id, 44);
}

#[test]
fn resolver_matches_czech_name_case_insensitively() {
    let resolved = resolve_lookup_in_indexes(
        &[(
            7,
            index(
                7,
                vec![entry(
                    42,
                    "IZP",
                    "Úvod do programování",
                    "2026-09-01",
                    "2027-01-31",
                    1,
                )],
            ),
        )],
        Some("úvod do programování"),
        None,
        "2026-09-30",
    )
    .expect("Czech name must match regardless of case");
    assert_eq!(resolved.identity.offering_id, 42);
}

#[test]
fn resolver_uses_full_unicode_case_folding_for_names() {
    let resolved = resolve_lookup_in_indexes(
        &[(
            7,
            index(
                7,
                vec![entry(42, "LANG", "Straße", "2026-09-01", "2027-01-31", 1)],
            ),
        )],
        Some("STRASSE"),
        None,
        "2026-09-30",
    )
    .expect("full Unicode caseless match");
    assert_eq!(resolved.identity.offering_id, 42);
}

#[test]
fn resolver_uses_current_then_latest_semester_and_rejects_ties() {
    let rows = vec![
        entry(41, "IZP", "Programming", "2025-09-01", "2026-01-31", 1),
        entry(42, "IZP", "Programming", "2026-09-01", "2027-01-31", 2),
    ];
    let current = resolve_lookup_in_indexes(
        &[(7, index(7, rows.clone()))],
        Some("IZP"),
        None,
        "2026-09-30",
    )
    .unwrap();
    assert_eq!(current.identity.offering_id, 42);
    let latest =
        resolve_lookup_in_indexes(&[(7, index(7, rows))], Some("IZP"), None, "2028-09-30").unwrap();
    assert_eq!(latest.identity.offering_id, 42);

    let tied = vec![
        entry(42, "IZP", "Programming", "2026-09-01", "2027-01-31", 1),
        entry(43, "IZP", "Programming", "2026-09-01", "2027-01-31", 2),
    ];
    let error = resolve_lookup_in_indexes(&[(7, index(7, tied))], Some("IZP"), None, "2028-09-30")
        .unwrap_err();
    assert!(error.contains("42, 43"));
    assert!(error.contains("--offering-id"));
}

#[test]
fn resolver_rejects_matches_in_multiple_active_studies() {
    let error = resolve_lookup_in_indexes(
        &[
            (
                7,
                index(
                    7,
                    vec![entry(
                        42,
                        "IZP",
                        "Programming",
                        "2026-09-01",
                        "2027-01-31",
                        1,
                    )],
                ),
            ),
            (
                8,
                index(
                    8,
                    vec![entry(
                        43,
                        "IZP",
                        "Programming",
                        "2026-09-01",
                        "2027-01-31",
                        2,
                    )],
                ),
            ),
        ],
        Some("IZP"),
        None,
        "2026-09-30",
    )
    .unwrap_err();
    assert!(error.contains("7, 8"));
    assert!(error.contains("--study-id"));
}

#[test]
fn active_study_markers_are_strict_and_empty_active_set_is_visible() {
    assert_eq!(
        active_study_ids(&json!({"data":{"studia":[
            {"studium_id":7,"aktivni_studium":1},
            {"studium_id":8,"aktivni_studium":0}
        ]}})),
        Ok(vec![7])
    );
    assert!(
        active_study_ids(&json!({"data":{"studia":[
            {"studium_id":7,"aktivni_studium":2}
        ]}}))
        .is_err()
    );
}

#[test]
fn active_resolution_fetches_every_index_and_never_selects_a_partial_subset() {
    let studies = json!({"data":{"studia":[
        {"studium_id":7,"aktivni_studium":1},
        {"studium_id":8,"aktivni_studium":1}
    ]}});
    let mut fetched = Vec::new();
    let error = resolve_active_indexes(&studies, Some("IZP"), None, "2026-09-30", |study_id| {
        fetched.push(study_id);
        if study_id == 8 {
            Err("synthetic index failure")
        } else {
            Ok(index(
                7,
                vec![entry(
                    42,
                    "IZP",
                    "Programming",
                    "2026-09-01",
                    "2027-01-31",
                    1,
                )],
            ))
        }
    })
    .unwrap_err();
    assert_eq!(fetched, vec![7, 8]);
    assert!(error.contains("failed: 8"));
    assert!(error.contains("no subject was selected"));
}

#[test]
fn active_resolution_stops_index_requests_after_rate_limit() {
    let studies = json!({"data":{"studia":[
        {"studium_id":7,"aktivni_studium":1},
        {"studium_id":8,"aktivni_studium":1}
    ]}});
    let mut fetched = Vec::new();
    let error = resolve_active_indexes(&studies, Some("IZP"), None, "2026-09-30", |study_id| {
        fetched.push(study_id);
        Err("VUT API rate limited")
    })
    .unwrap_err();
    assert_eq!(fetched, vec![7]);
    assert!(error.contains("rate limited"));
}

#[test]
fn vut_today_uses_prague_date_at_utc_midnight_boundary() {
    let instant = UNIX_EPOCH + Duration::from_secs(1_790_807_400);
    assert_eq!(vut_today_at(instant), "2026-10-01");
}

#[test]
fn shorthand_defaults_cover_academic_year_and_allow_individual_overrides() {
    let resolved = resolve_lookup_in_indexes(
        &[(
            7,
            index(
                7,
                vec![entry(
                    42,
                    "IZP",
                    "Programming",
                    "2027-02-01",
                    "2027-06-30",
                    1,
                )],
            ),
        )],
        Some("IZP"),
        None,
        "2027-02-10",
    )
    .unwrap();
    let base = SubjectRequest {
        code_or_name: Some("IZP".into()),
        offering_id: None,
        study_id: None,
        from: Some("2026-10-01T00:00".into()),
        to: None,
        news_since: None,
        max_news: 0,
    };
    let scope = scope_from_request(&base, &resolved).unwrap();
    assert_eq!(scope.from, "2026-10-01T00:00");
    assert_eq!(scope.to, "2027-08-31T23:59");
    assert_eq!(scope.news_since, "1900-01-01");
    assert_eq!(scope.max_news, 0);
}

#[test]
fn news_selection_uses_both_study_and_offering_and_bounds_hydration() {
    let list = json!({"data":{"dokumenty":[
            {"studium_id":7,"aktualni_predmet_id":42,"aktualita_id":3,"nadpis":"A","datum_vystaveni":"2026-09-29 08:00:00"},
        {"studium_id":7,"aktualni_predmet_id":43,"aktualita_id":4,"nadpis":"other course"},
        {"studium_id":8,"aktualni_predmet_id":42,"aktualita_id":5,"nadpis":"other study"},
            {"studium_id":7,"aktualni_predmet_id":42,"aktualita_id":6,"nadpis":"B","datum_vystaveni":"2026-09-30 08:00:00"}
    ]}});
    let selected = select_news(&list, 7, 42, 1).expect("select news");
    assert_eq!(selected.items.len(), 1);
    assert_eq!(selected.total_matching, 2);
    assert!(selected.truncated);
    assert_eq!(selected.items[0]["aktualita_id"], 6);
    let all = select_news(&list, 7, 42, 0).expect("all returned news");
    assert_eq!(all.items.len(), 2);
    assert!(!all.truncated);
}

#[test]
fn partial_sections_distinguish_empty_from_unverified_or_failed() {
    let scope = RequestScope {
        offering_id: 42,
        study_id: 7,
        from: "2026-09-28T00:00".into(),
        to: "2026-10-05T23:59".into(),
        news_since: "2026-09-01".into(),
        max_news: 10,
    };
    let personal = schedule_section(&scope, Ok(json!({"data":{}})));
    let course = timetable_section(&scope, Ok(json!({"data":{}})));
    let denied = schedule_section(&scope, Err("VUT API access denied"));
    assert_eq!(serde_json::to_value(personal).unwrap()["status"], "empty");
    assert_eq!(
        serde_json::to_value(course).unwrap()["reason"],
        "unverified_empty_response"
    );
    assert_eq!(
        serde_json::to_value(denied).unwrap()["reason"],
        "permission_denied"
    );
}

#[test]
fn returned_moodle_link_is_checked_before_exposure() {
    let identity = SubjectIdentity {
        offering_id: 42,
        subject_id: 9,
        faculty_id: 13,
        academic_year: 2026,
        semester_type_id: 2,
        study_id: 7,
    };
    let good = moodle_section(
        &identity,
        Ok(
            json!({"data":{"odkazy":[{"odkaz_moodle":"https://moodle.vut.cz/course/view.php?id=42"}]}}),
        ),
    );
    let bad = moodle_section(
        &identity,
        Ok(json!({"data":{"odkazy":[{"odkaz_moodle":"https://moodle.vut.cz.evil.example/path"}]}})),
    );
    let ambiguous = moodle_section(
        &identity,
        Ok(json!({"data":{"odkazy":[
            {"odkaz_moodle":"https://moodle.vut.cz/course/view.php?id=42"},
            {"odkaz_moodle":"https://moodle.vut.cz/course/view.php?id=43"}
        ]}})),
    );
    let good = serde_json::to_value(good).unwrap();
    assert_eq!(good["reason"], "web_session_unavailable");
    assert_eq!(
        good["data"]["course_url"],
        "https://moodle.vut.cz/course/view.php?id=42"
    );
    assert_eq!(
        serde_json::to_value(bad).unwrap()["reason"],
        "no_verified_moodle_link"
    );
    assert_eq!(
        serde_json::to_value(ambiguous).unwrap()["reason"],
        "ambiguous_moodle_links"
    );
}

#[test]
fn web_auth_failure_preserves_api_moodle_failure_without_link() {
    let mut missing = Section::unavailable(None, "no_verified_moodle_link", Value::Null);
    mark_moodle_web_auth_required(&mut missing);
    assert_eq!(missing.reason, Some("no_verified_moodle_link"));

    let mut limited = Section::unavailable(None, "rate_limited", Value::Null);
    mark_moodle_web_auth_required(&mut limited);
    assert_eq!(limited.reason, Some("rate_limited"));

    let mut linked = Section::unavailable(
        None,
        "web_session_unavailable",
        json!({"course_url":"https://moodle.vut.cz/course/view.php?id=42"}),
    );
    mark_moodle_web_auth_required(&mut linked);
    assert_eq!(linked.reason, Some("auth_required"));
}

#[test]
fn studis_auth_failure_does_not_hide_an_available_moodle_course() {
    let scope = RequestScope {
        offering_id: 42,
        study_id: 7,
        from: "2026-09-01T00:00".into(),
        to: "2027-08-31T23:59".into(),
        news_since: "1900-01-01".into(),
        max_news: 0,
    };
    let empty = || Section::unavailable(None, "synthetic", Value::Null);
    let mut view = SubjectView {
        schema_version: 1,
        subject: SubjectIdentity {
            offering_id: 42,
            subject_id: 142,
            faculty_id: 13,
            academic_year: 2026,
            semester_type_id: 2,
            study_id: 7,
        },
        requested: RequestedScope {
            from: scope.from.clone(),
            to: scope.to.clone(),
            news_since: scope.news_since.clone(),
            max_news: scope.max_news,
        },
        sections: SubjectSections {
            catalog: empty(),
            study_record: empty(),
            announcements: empty(),
            personal_schedule: empty(),
            course_timetable: empty(),
            moodle: Section::unavailable(
                None,
                "web_session_unavailable",
                json!({"course_url":"https://moodle.vut.cz/course/view.php?id=42","sections":[]}),
            ),
        },
        warnings: Vec::new(),
    };

    enrich_web_with(&mut view, &scope, |url| {
        if url.starts_with("https://www.vut.cz/") {
            Err(WebError::AuthRequired)
        } else if url == "https://moodle.vut.cz/course/view.php?id=42" {
            Ok("<body id='page-course-view-topics'><ul><li class='section' data-sectionid='1'><h3 class='sectionname'>Files</h3></li></ul></body>".into())
        } else {
            panic!("unexpected read: {url}");
        }
    });

    let output = serde_json::to_value(view).unwrap();
    assert_eq!(output["sections"]["moodle"]["status"], "available");
    assert_eq!(
        output["sections"]["moodle"]["data"]["sections"][0]["title"],
        "Files"
    );
}

#[test]
fn rejected_auth_or_rate_limit_stops_later_source_requests() {
    let mut guard = FetchGuard::default();
    assert_eq!(
        guard.run(|| Err::<Value, _>("VUT API rate limited")),
        Err("VUT API rate limited")
    );
    assert_eq!(
        guard.run::<Value>(|| panic!("must not send another GET")),
        Err("VUT API rate limited")
    );

    let mut guard = FetchGuard::default();
    assert_eq!(
        guard.run(|| Err::<Value, _>("VUT API authentication rejected")),
        Err("VUT API authentication rejected")
    );
    assert_eq!(
        guard.run::<Value>(|| panic!("must not retry rejected auth")),
        Err("VUT API authentication rejected")
    );
}

#[test]
fn expired_detail_budget_skips_network() {
    let mut guard = FetchGuard::default();
    let expired = std::time::Instant::now() - std::time::Duration::from_millis(1);
    assert_eq!(
        fetch_detail_with_budget::<Value>(&mut guard, expired, || panic!("must not GET")),
        Err("subject news detail budget exhausted")
    );
}

#[test]
fn news_budget_marks_every_unhydrated_returned_row_and_completeness_false() {
    let scope = RequestScope {
        offering_id: 42,
        study_id: 7,
        from: "2026-09-01T00:00".into(),
        to: "2027-08-31T23:59".into(),
        news_since: "1900-01-01".into(),
        max_news: 0,
    };
    let list = Ok(json!({"data":{"dokumenty":[
        {"studium_id":7,"aktualni_predmet_id":42,"aktualita_id":1},
        {"studium_id":7,"aktualni_predmet_id":42,"aktualita_id":2}
    ]}}));
    let mut guard = FetchGuard::default();
    let section = announcement_section_with(
        &scope,
        list,
        &mut guard,
        Instant::now() - Duration::from_millis(1),
        |_| panic!("expired budget must skip detail GETs"),
    );
    let output = serde_json::to_value(section).unwrap();
    assert_eq!(output["data"]["returned_count"], 2);
    assert_eq!(output["data"]["matching_count"], 2);
    assert_eq!(output["data"]["truncated"], false);
    assert_eq!(output["data"]["complete"], false);
    assert!(
        output["data"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["reason"] == "budget_exhausted")
    );
}

#[test]
fn news_rate_limit_stops_further_details_and_stays_explicitly_partial() {
    let scope = RequestScope {
        offering_id: 42,
        study_id: 7,
        from: "2026-09-01T00:00".into(),
        to: "2027-08-31T23:59".into(),
        news_since: "1900-01-01".into(),
        max_news: 0,
    };
    let list = Ok(json!({"data":{"dokumenty":[
        {"studium_id":7,"aktualni_predmet_id":42,"aktualita_id":1},
        {"studium_id":7,"aktualni_predmet_id":42,"aktualita_id":2}
    ]}}));
    let mut calls = 0;
    let mut guard = FetchGuard::default();
    let section = announcement_section_with(
        &scope,
        list,
        &mut guard,
        Instant::now() + Duration::from_secs(1),
        |_| {
            calls += 1;
            Err("VUT API rate limited")
        },
    );
    let output = serde_json::to_value(section).unwrap();
    assert_eq!(calls, 1);
    assert_eq!(output["data"]["complete"], false);
    assert!(
        output["data"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["reason"] == "rate_limited")
    );
}

#[test]
fn web_timetable_only_claims_empty_or_available_when_window_is_covered() {
    let mut section = Section::unavailable(None, "unverified_empty_response", Value::Null);
    apply_timetable_html(
        &mut section,
        json!({"covered_from":"2026-09-28","covered_to":"2026-10-02","window_covered":false,"events":[]}),
        "https://www.vut.cz/studis/student.phtml".into(),
    );
    assert_eq!(
        serde_json::to_value(&section).unwrap()["reason"],
        "calendar_window_not_covered"
    );
    apply_timetable_html(
        &mut section,
        json!({"covered_from":"2026-09-28","covered_to":"2026-10-05","window_covered":true,"times_verified":true,"events":[]}),
        "https://www.vut.cz/studis/student.phtml".into(),
    );
    assert_eq!(serde_json::to_value(&section).unwrap()["status"], "empty");
}
