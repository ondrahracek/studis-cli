//! Authenticated Studis and Moodle enrichment for a composed subject view.

use serde_json::{Value, json};
use std::time::{Duration, Instant};

use super::{RequestScope, Section, SectionStatus, Source, SubjectIdentity, SubjectView};
use crate::{
    web::{moodle, studis},
    web_session::{WebError, WebSession},
};

pub(super) fn mark_moodle_web_auth_required(section: &mut Section) {
    if section.data.get("course_url").is_some() {
        section.reason = Some("auth_required");
    }
}

fn web_timetable_url(identity: &SubjectIdentity) -> String {
    format!(
        "https://www.vut.cz/studis/student.phtml?gm=gm_rozvrh_predmetu&operation=rozvrh&predmet_id={}&fakulta_id={}&aktualni_rok={}&typ_semestru_id={}",
        identity.subject_id, identity.faculty_id, identity.academic_year, identity.semester_type_id
    )
}

pub(super) fn apply_timetable_html(section: &mut Section, data: Value, url: String) {
    let covered = data.get("window_covered").and_then(Value::as_bool) == Some(true);
    let times_verified = data.get("times_verified").and_then(Value::as_bool) == Some(true);
    let has_events = data
        .get("events")
        .and_then(Value::as_array)
        .is_some_and(|events| !events.is_empty());
    section.sources.push(Source::now(url));
    section.data = data;
    if covered && times_verified {
        section.status = if has_events {
            SectionStatus::Available
        } else {
            SectionStatus::Empty
        };
        section.reason = None;
    } else {
        section.status = SectionStatus::Unavailable;
        section.reason = Some(if covered {
            "event_time_unverified"
        } else {
            "calendar_window_not_covered"
        });
    }
    section
        .limitations
        .push("course_wide_schedule_may_include_other_groups");
}

pub(super) fn enrich_web(view: &mut SubjectView, scope: &RequestScope) {
    let session = match WebSession::open(false, false) {
        Ok(session) => session,
        Err(error) => {
            if view.sections.moodle.data.get("course_url").is_some() {
                view.sections.moodle.reason = Some(error.reason());
            }
            return;
        }
    };

    let catalog_url = format!(
        "https://www.vut.cz/studis/student.phtml?gm=gm_detail_predmetu&apid={}",
        scope.offering_id
    );
    match session.read(&catalog_url) {
        Ok(html) => match studis::parse_catalogue(&html, &catalog_url) {
            Ok(data) => {
                if let Some(map) = view.sections.catalog.data.as_object_mut() {
                    map.insert("rich_fields".into(), data["fields"].clone());
                }
                view.sections.catalog.sources.push(Source::now(catalog_url));
                view.sections
                    .catalog
                    .limitations
                    .retain(|limit| *limit != "rich_catalogue_text_requires_web_access");
            }
            Err(_) => view
                .sections
                .catalog
                .limitations
                .push("rich_catalogue_web_parse_failed"),
        },
        Err(WebError::AuthRequired) => {
            mark_moodle_web_auth_required(&mut view.sections.moodle);
            view.sections.catalog.limitations.push("web_auth_required");
            view.sections
                .study_record
                .limitations
                .push("web_auth_required");
            return;
        }
        Err(_) => view
            .sections
            .catalog
            .limitations
            .push("rich_catalogue_web_fetch_failed"),
    }

    let personal_url = format!(
        "https://www.vut.cz/studis/student.phtml?sn=predmet_detail&apid={}",
        scope.offering_id
    );
    if let Ok(html) = session.read(&personal_url)
        && let Ok(data) = studis::parse_personal_detail(&html)
    {
        if let Some(map) = view.sections.study_record.data.as_object_mut() {
            map.insert("personal_fields".into(), data["fields"].clone());
            map.insert("assessments".into(), data["assessments"].clone());
        }
        view.sections
            .study_record
            .sources
            .push(Source::now(personal_url));
        view.sections
            .study_record
            .limitations
            .retain(|limit| *limit != "assessment_table_may_require_web_access");
    }

    let deadline = Instant::now() + Duration::from_secs(60);
    let announcements = &mut view.sections.announcements;
    if let Some(items) = announcements
        .data
        .get_mut("items")
        .and_then(Value::as_array_mut)
    {
        for item in items {
            let Some(url) = item
                .get("web_url")
                .and_then(Value::as_str)
                .map(str::to_owned)
            else {
                continue;
            };
            if Instant::now() >= deadline {
                if let Some(map) = item.as_object_mut() {
                    map.insert("web_status".into(), json!("unavailable"));
                    map.insert("web_reason".into(), json!("budget_exhausted"));
                }
                continue;
            }
            match session.read(&url) {
                Ok(html) => match studis::parse_announcement_detail(&html, &url) {
                    Ok(data) => {
                        if let Some(map) = item.as_object_mut() {
                            map.insert("web_status".into(), json!("available"));
                            map.insert("web_detail".into(), data);
                        }
                        announcements.sources.push(Source::now(url));
                    }
                    Err(_) => {
                        if let Some(map) = item.as_object_mut() {
                            map.insert("web_status".into(), json!("unavailable"));
                            map.insert("web_reason".into(), json!("invalid_web_page"));
                        }
                    }
                },
                Err(error) => {
                    if let Some(map) = item.as_object_mut() {
                        map.insert("web_status".into(), json!("unavailable"));
                        map.insert("web_reason".into(), json!(error.reason()));
                    }
                }
            }
        }
    }
    if announcements
        .data
        .get("items")
        .and_then(Value::as_array)
        .is_some_and(|items| {
            items
                .iter()
                .all(|item| item.get("web_status") == Some(&json!("available")))
        })
    {
        announcements
            .limitations
            .retain(|limit| *limit != "news_api_may_omit_web_body_links");
    }

    if matches!(
        view.sections.course_timetable.reason,
        Some("unverified_empty_response")
    ) {
        let url = web_timetable_url(&view.subject);
        if let Ok(html) = session.read(&url)
            && let Ok(data) = studis::parse_timetable(&html, &scope.from, &scope.to)
        {
            apply_timetable_html(&mut view.sections.course_timetable, data, url);
        }
    }

    let course_url = view
        .sections
        .moodle
        .data
        .get("course_url")
        .and_then(Value::as_str)
        .map(str::to_owned);
    if let Some(url) = course_url {
        match session.read(&url) {
            Ok(html) => match moodle::parse_overview(&html, &url) {
                Ok(data) => {
                    view.sections.moodle.status = SectionStatus::Available;
                    view.sections.moodle.reason = None;
                    view.sections.moodle.data =
                        json!({"course_url":url,"sections":data["sections"]});
                    view.sections.moodle.sources.push(Source::now(url));
                }
                Err(_) => view.sections.moodle.reason = Some("invalid_web_page"),
            },
            Err(error) => view.sections.moodle.reason = Some(error.reason()),
        }
    }
}
