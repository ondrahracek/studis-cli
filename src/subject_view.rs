//! One-subject composition from the current user's read-only VUT sources.

use serde::Serialize;
use serde_json::{Value, json};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::resources::{news, schedule, studies, subjects};

mod lookup;
mod web;

#[cfg(test)]
use lookup::{active_study_ids, resolve_active_indexes, resolve_lookup_in_indexes, vut_today_at};
use lookup::{resolve_subject, scope_from_request, vut_today};
use web::enrich_web;
#[cfg(test)]
use web::{apply_timetable_html, mark_moodle_web_auth_required};

#[derive(Clone, Debug)]
pub(crate) struct SubjectRequest {
    pub code_or_name: Option<String>,
    pub offering_id: Option<u64>,
    pub study_id: Option<u64>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub news_since: Option<String>,
    /// Zero means every matching row returned by the news-list response.
    pub max_news: usize,
}

#[derive(Clone, Debug)]
pub(crate) struct RequestScope {
    pub offering_id: u64,
    pub study_id: u64,
    pub from: String,
    pub to: String,
    pub news_since: String,
    pub max_news: usize,
}

#[derive(Serialize)]
pub(crate) struct SubjectView {
    schema_version: u8,
    subject: SubjectIdentity,
    requested: RequestedScope,
    sections: SubjectSections,
    warnings: Vec<&'static str>,
}

#[derive(Serialize)]
struct RequestedScope {
    from: String,
    to: String,
    news_since: String,
    max_news: usize,
}

#[derive(Serialize)]
struct SubjectSections {
    catalog: Section,
    study_record: Section,
    announcements: Section,
    personal_schedule: Section,
    course_timetable: Section,
    moodle: Section,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum SectionStatus {
    Available,
    Empty,
    Unavailable,
}

#[derive(Serialize)]
struct Source {
    url: String,
    fetched_at_unix_ms: u128,
}

impl Source {
    fn now(url: String) -> Self {
        let fetched_at_unix_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        Self {
            url,
            fetched_at_unix_ms,
        }
    }
}

#[derive(Serialize)]
struct Section {
    status: SectionStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<&'static str>,
    sources: Vec<Source>,
    data: Value,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    limitations: Vec<&'static str>,
}

impl Section {
    fn available(source: Source, data: Value) -> Self {
        Self {
            status: SectionStatus::Available,
            reason: None,
            sources: vec![source],
            data,
            limitations: Vec::new(),
        }
    }

    fn empty(source: Source, data: Value) -> Self {
        Self {
            status: SectionStatus::Empty,
            reason: None,
            sources: vec![source],
            data,
            limitations: Vec::new(),
        }
    }

    fn unavailable(source: Option<Source>, reason: &'static str, data: Value) -> Self {
        Self {
            status: SectionStatus::Unavailable,
            reason: Some(reason),
            sources: source.into_iter().collect(),
            data,
            limitations: Vec::new(),
        }
    }

    fn limitation(mut self, limitation: &'static str) -> Self {
        self.limitations.push(limitation);
        self
    }
}

fn reason(error: &'static str) -> &'static str {
    match error {
        "VUT API credentials are missing"
        | "VUT API authentication rejected"
        | "VUT API access token is empty" => "auth_required",
        "VUT API access denied" => "permission_denied",
        "VUT API rate limited" | "VUT authentication rate limited" => "rate_limited",
        "subject news detail budget exhausted" => "budget_exhausted",
        error if error.starts_with("invalid VUT ") => "invalid_response",
        _ => "fetch_failed",
    }
}

#[derive(Default)]
struct FetchGuard {
    blocked: Option<&'static str>,
}

impl FetchGuard {
    fn run<T>(
        &mut self,
        fetch: impl FnOnce() -> Result<T, &'static str>,
    ) -> Result<T, &'static str> {
        if let Some(error) = self.blocked {
            return Err(error);
        }
        let result = fetch();
        if let Err(&error) = result.as_ref()
            && matches!(
                error,
                "VUT API authentication rejected"
                    | "VUT API rate limited"
                    | "VUT authentication rate limited"
            )
        {
            self.blocked = Some(error);
        }
        result
    }
}

fn fetch_detail_with_budget<T>(
    guard: &mut FetchGuard,
    deadline: Instant,
    fetch: impl FnOnce() -> Result<T, &'static str>,
) -> Result<T, &'static str> {
    if let Some(error) = guard.blocked {
        return Err(error);
    }
    if Instant::now() >= deadline {
        return Err("subject news detail budget exhausted");
    }
    guard.run(fetch)
}

fn source_url(request: reqwest::blocking::Request) -> String {
    request.url().to_string()
}

fn url_for(
    build: impl FnOnce(&reqwest::blocking::Client) -> Result<reqwest::blocking::Request, &'static str>,
) -> String {
    source_url(build(&reqwest::blocking::Client::new()).expect("fixed API URL is valid"))
}

fn catalog_section(identity: &SubjectIdentity, catalog: Result<Value, &'static str>) -> Section {
    let source = Source::now(url_for(|client| {
        subjects::catalog_request(client, "source-only", identity.offering_id)
    }));
    match catalog {
        Err(error) => Section::unavailable(Some(source), reason(error), Value::Null),
        Ok(raw) => {
            let rows = raw
                .pointer("/data/predmety")
                .and_then(Value::as_array)
                .expect("validated catalogue");
            let matching: Vec<_> = rows
                .iter()
                .filter(|row| {
                    row.get("predmet_id").and_then(Value::as_u64) == Some(identity.subject_id)
                })
                .filter(|row| {
                    row.get("fakulta_id").and_then(Value::as_u64) == Some(identity.faculty_id)
                })
                .filter(|row| {
                    row.get("aktualni_predmety")
                        .and_then(Value::as_array)
                        .is_some_and(|offerings| {
                            offerings.iter().any(|offering| {
                                offering.get("aktualni_predmet_id").and_then(Value::as_u64)
                                    == Some(identity.offering_id)
                            })
                        })
                })
                .cloned()
                .collect();
            if matching.is_empty() {
                return Section::unavailable(Some(source), "identity_mismatch", Value::Null);
            }
            Section::available(source,json!({"records":matching,"web_detail_url":format!("https://www.vut.cz/studis/student.phtml?gm=gm_detail_predmetu&apid={}",identity.offering_id)}))
                .limitation("rich_catalogue_text_requires_web_access")
        }
    }
}

fn schedule_section(scope: &RequestScope, result: Result<Value, &'static str>) -> Section {
    let source = Source::now(url_for(|client| {
        schedule::teaching_request(client, "source-only", &scope.from, &scope.to)
    }));
    match result {
        Err(error) => Section::unavailable(Some(source), reason(error), Value::Null),
        Ok(raw) => {
            let rows = raw
                .pointer("/data/vyucovani")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let entries: Vec<_> = rows
                .into_iter()
                .filter(|row| {
                    row.get("v_aktualni_predmet_id").and_then(Value::as_u64)
                        == Some(scope.offering_id)
                })
                .collect();
            if entries.is_empty() {
                Section::empty(source, json!({"entries":[]}))
            } else {
                Section::available(source, json!({"entries":entries}))
            }
        }
    }
}

fn timetable_section(scope: &RequestScope, result: Result<Value, &'static str>) -> Section {
    let source = Source::now(url_for(|client| {
        subjects::timetable_request(
            client,
            "source-only",
            scope.offering_id,
            &scope.from,
            &scope.to,
        )
    }));
    match result {
        Err(error) => Section::unavailable(Some(source), reason(error), Value::Null),
        Ok(raw) => {
            let entries = raw
                .pointer("/data/vyucovani")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            if entries.is_empty() {
                Section::unavailable(Some(source), "unverified_empty_response", Value::Null)
            } else {
                Section::available(source, json!({"entries":entries}))
                    .limitation("course_wide_schedule_may_include_other_groups")
            }
        }
    }
}

fn moodle_section(identity: &SubjectIdentity, result: Result<Value, &'static str>) -> Section {
    let source = Source::now(url_for(|client| {
        subjects::moodle_request(client, "source-only", identity.offering_id)
    }));
    match result {
        Err(error) => Section::unavailable(Some(source), reason(error), Value::Null),
        Ok(raw) => {
            let links = raw
                .pointer("/data/odkazy")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let url = links
                .iter()
                .filter_map(|link| link.get("odkaz_moodle").and_then(Value::as_str))
                .find(|url| {
                    reqwest::Url::parse(url).is_ok_and(|parsed| {
                        parsed.scheme() == "https"
                            && parsed.host_str() == Some("moodle.vut.cz")
                            && parsed.username().is_empty()
                            && parsed.password().is_none()
                    })
                });
            match url {
                Some(url) => Section::unavailable(
                    Some(source),
                    "web_session_unavailable",
                    json!({"course_url":url,"sections":[]}),
                ),
                None => Section::unavailable(Some(source), "no_verified_moodle_link", Value::Null),
            }
        }
    }
}

fn announcement_section(
    scope: &RequestScope,
    result: Result<Value, &'static str>,
    guard: &mut FetchGuard,
) -> Section {
    announcement_section_with(
        scope,
        result,
        guard,
        Instant::now() + Duration::from_secs(60),
        news::fetch_detail,
    )
}

fn announcement_section_with(
    scope: &RequestScope,
    result: Result<Value, &'static str>,
    guard: &mut FetchGuard,
    deadline: Instant,
    mut detail_fetch: impl FnMut(u64) -> Result<Value, &'static str>,
) -> Section {
    let list_source = Source::now(url_for(|client| {
        news::request(client, "source-only", &scope.news_since)
    }));
    let raw = match result {
        Ok(raw) => raw,
        Err(error) => return Section::unavailable(Some(list_source), reason(error), Value::Null),
    };
    let selected = match select_news(&raw, scope.study_id, scope.offering_id, scope.max_news) {
        Ok(selected) => selected,
        Err(error) => return Section::unavailable(Some(list_source), reason(error), Value::Null),
    };
    let mut sources = vec![list_source];
    let mut items = Vec::new();
    for summary in selected.items {
        let id = summary.get("aktualita_id").and_then(Value::as_u64);
        let Some(id) = id else {
            items.push(
                json!({"summary":summary,"detail_status":"unavailable","reason":"invalid_news_id"}),
            );
            continue;
        };
        let detail_result = fetch_detail_with_budget(guard, deadline, || {
            sources.push(Source::now(url_for(|client| {
                news::detail_request(client, "source-only", id)
            })));
            detail_fetch(id)
        });
        let detail = match detail_result {
            Ok(raw) => raw
                .pointer("/data/dokumenty")
                .and_then(Value::as_array)
                .and_then(|rows| {
                    rows.iter().find(|row| {
                        row.get("aktualita_id").and_then(Value::as_u64) == Some(id)
                            && row.get("studium_id").and_then(Value::as_u64) == Some(scope.study_id)
                            && row.get("aktualni_predmet_id").and_then(Value::as_u64)
                                == Some(scope.offering_id)
                    })
                })
                .cloned(),
            Err(error) => {
                items.push(json!({"summary":summary,"detail_status":"unavailable","reason":reason(error),"web_url":format!("https://www.vut.cz/studis/student.phtml?sn=aktuality_predmet&akce=2&did={id}&apid={}",scope.offering_id)}));
                continue;
            }
        };
        match detail {
            Some(detail)=>items.push(json!({"summary":summary,"detail_status":"available","detail":detail,"web_url":format!("https://www.vut.cz/studis/student.phtml?sn=aktuality_predmet&akce=2&did={id}&apid={}",scope.offering_id)})),
            None=>items.push(json!({"summary":summary,"detail_status":"unavailable","reason":"identity_mismatch","web_url":format!("https://www.vut.cz/studis/student.phtml?sn=aktuality_predmet&akce=2&did={id}&apid={}",scope.offering_id)})),
        }
    }
    let status = if selected.total_matching == 0 {
        SectionStatus::Empty
    } else {
        SectionStatus::Available
    };
    let complete = !selected.truncated
        && items
            .iter()
            .all(|item| item.get("detail_status").and_then(Value::as_str) == Some("available"));
    Section {
        status,
        reason: None,
        sources,
        data: json!({"items":items,"returned_count":items.len(),"matching_count":selected.total_matching,"truncated":selected.truncated,"complete":complete}),
        limitations: vec!["news_api_may_omit_web_body_links"],
    }
}

fn compose(scope: RequestScope, identity: SubjectIdentity, records: Vec<Value>) -> SubjectView {
    let mut guard = FetchGuard::default();
    let study_record = Section::available(
        Source::now(url_for(|client| {
            studies::index_request(client, "source-only", scope.study_id)
        })),
        json!({"index_entries":records}),
    )
    .limitation("assessment_table_may_require_web_access");
    let catalog = catalog_section(
        &identity,
        guard.run(|| subjects::fetch_catalog(scope.offering_id)),
    );
    let personal_schedule = schedule_section(
        &scope,
        guard.run(|| schedule::fetch_teaching(&scope.from, &scope.to)),
    );
    let course_timetable = timetable_section(
        &scope,
        guard.run(|| subjects::fetch_timetable(scope.offering_id, &scope.from, &scope.to)),
    );
    let moodle = moodle_section(
        &identity,
        guard.run(|| subjects::fetch_moodle(scope.offering_id)),
    );
    let news_result = guard.run(|| news::fetch(&scope.news_since));
    let announcements = announcement_section(&scope, news_result, &mut guard);
    let mut view = SubjectView {
        schema_version: 1,
        subject: identity,
        requested: RequestedScope {
            from: scope.from.clone(),
            to: scope.to.clone(),
            news_since: scope.news_since.clone(),
            max_news: scope.max_news,
        },
        sections: SubjectSections {
            catalog,
            study_record,
            announcements,
            personal_schedule,
            course_timetable,
            moodle,
        },
        warnings: Vec::new(),
    };
    enrich_web(&mut view, &scope);
    view
}

pub(crate) fn fetch(request: SubjectRequest) -> Result<SubjectView, String> {
    let resolved = resolve_subject(&request, &vut_today())?;
    let scope = scope_from_request(&request, &resolved)?;
    Ok(compose(scope, resolved.identity, resolved.records))
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct SubjectIdentity {
    pub offering_id: u64,
    pub subject_id: u64,
    pub faculty_id: u64,
    pub academic_year: u64,
    pub semester_type_id: u64,
    pub study_id: u64,
}

fn required_id(record: &Value, key: &str) -> Result<u64, &'static str> {
    record
        .get(key)
        .and_then(Value::as_u64)
        .ok_or("invalid VUT subject identity")
}

fn identity_from_record(
    record: &Value,
    study_id: u64,
    offering_id: u64,
) -> Result<SubjectIdentity, &'static str> {
    Ok(SubjectIdentity {
        offering_id,
        subject_id: required_id(record, "predmet_id")?,
        faculty_id: required_id(record, "fakulta_id")?,
        academic_year: required_id(record, "akrok")?,
        semester_type_id: required_id(record, "typ_semestru_id")?,
        study_id,
    })
}

struct SelectedNews {
    items: Vec<Value>,
    total_matching: usize,
    truncated: bool,
}

fn select_news(
    list: &Value,
    study_id: u64,
    offering_id: u64,
    max_news: usize,
) -> Result<SelectedNews, &'static str> {
    let data = list
        .get("data")
        .and_then(Value::as_object)
        .ok_or("invalid VUT news response")?;
    let rows = match data.get("dokumenty") {
        Some(value) => value.as_array().ok_or("invalid VUT news response")?,
        None if data.is_empty() => {
            return Ok(SelectedNews {
                items: Vec::new(),
                total_matching: 0,
                truncated: false,
            });
        }
        None => return Err("invalid VUT news response"),
    };
    let matching = rows.iter().filter(|row| {
        row.get("studium_id").and_then(Value::as_u64) == Some(study_id)
            && row.get("aktualni_predmet_id").and_then(Value::as_u64) == Some(offering_id)
    });
    let mut matching: Vec<_> = matching.cloned().collect();
    matching.sort_by(|a, b| {
        b.get("datum_vystaveni")
            .and_then(Value::as_str)
            .cmp(&a.get("datum_vystaveni").and_then(Value::as_str))
    });
    let total_matching = matching.len();
    let items = if max_news == 0 {
        matching
    } else {
        matching.into_iter().take(max_news).collect::<Vec<_>>()
    };
    Ok(SelectedNews {
        truncated: total_matching > items.len(),
        items,
        total_matching,
    })
}

#[cfg(test)]
mod tests;
