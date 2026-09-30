//! Study, offering, and default date-scope resolution.

use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    time::SystemTime,
};

use super::{RequestScope, SubjectIdentity, SubjectRequest, identity_from_record};
use crate::resources::studies;

#[derive(Debug)]
pub(super) struct ResolvedLookup {
    pub(super) identity: SubjectIdentity,
    pub(super) records: Vec<Value>,
}

fn index_entries(index: &Value, study_id: u64) -> Result<Vec<Value>, String> {
    let data = index
        .get("data")
        .and_then(Value::as_object)
        .ok_or_else(|| "invalid VUT study index response".to_owned())?;
    let groups = match data.get("studia") {
        Some(value) => value
            .as_array()
            .ok_or_else(|| "invalid VUT study index response".to_owned())?,
        None if data.is_empty() => return Ok(Vec::new()),
        None => return Err("invalid VUT study index response".into()),
    };
    let mut entries = Vec::new();
    for group in groups {
        if group.get("studium_id").and_then(Value::as_u64) != Some(study_id) {
            continue;
        }
        entries.extend(
            group
                .get("index")
                .and_then(Value::as_array)
                .ok_or_else(|| "invalid VUT study index response".to_owned())?
                .iter()
                .cloned(),
        );
    }
    Ok(entries)
}

fn row_matches(row: &Value, key: &str, query: &str) -> bool {
    row.get(key)
        .and_then(Value::as_str)
        .is_some_and(|value| caseless::canonical_caseless_match_str(value, query))
}

fn semester_date(row: &Value, key: &str) -> Result<String, String> {
    let value = row
        .get(key)
        .and_then(Value::as_str)
        .and_then(|value| value.get(..10))
        .ok_or_else(|| "invalid VUT subject semester interval".to_owned())?;
    crate::dates::date(value).map_err(|_| "invalid VUT subject semester interval".to_owned())
}

fn offering_interval(records: &[Value]) -> Result<(String, String), String> {
    let first = records
        .first()
        .ok_or_else(|| "subject offering is absent from the selected study".to_owned())?;
    let interval = (
        semester_date(first, "zacatek_semestru")?,
        semester_date(first, "konec_semestru")?,
    );
    if interval.0 > interval.1 {
        return Err("invalid VUT subject semester interval".into());
    }
    for record in &records[1..] {
        if semester_date(record, "zacatek_semestru")? != interval.0
            || semester_date(record, "konec_semestru")? != interval.1
        {
            return Err("conflicting VUT subject semester interval".into());
        }
    }
    Ok(interval)
}

fn ambiguity_message(kind: &str, ids: impl IntoIterator<Item = u64>, flag: &str) -> String {
    let values = ids
        .into_iter()
        .map(|id| id.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    format!("multiple {kind} match ({values}); select one with {flag}")
}

pub(super) fn resolve_lookup_in_indexes(
    indexes: &[(u64, Value)],
    query: Option<&str>,
    offering_filter: Option<u64>,
    today: &str,
) -> Result<ResolvedLookup, String> {
    crate::dates::date(today).map_err(|_| "invalid lookup date".to_owned())?;
    let mut all_rows = Vec::new();
    for (study_id, index) in indexes {
        for row in index_entries(index, *study_id)? {
            all_rows.push((*study_id, row));
        }
    }

    let mut matching = if let Some(query) = query {
        let codes = all_rows
            .iter()
            .filter(|(_, row)| row_matches(row, "zkratka", query))
            .cloned()
            .collect::<Vec<_>>();
        if codes.is_empty() {
            all_rows
                .into_iter()
                .filter(|(_, row)| row_matches(row, "ap_nazev", query))
                .collect::<Vec<_>>()
        } else {
            codes
        }
    } else {
        all_rows
    };
    if let Some(offering_id) = offering_filter {
        matching.retain(|(_, row)| {
            row.get("aktualni_predmet_id").and_then(Value::as_u64) == Some(offering_id)
        });
    }
    if matching.is_empty() {
        return Err(if offering_filter.is_some() && query.is_some() {
            "subject code or name does not match --offering-id; verify the selectors".into()
        } else {
            "subject was not found in active studies; use --study-id for historical access".into()
        });
    }

    let study_ids = matching
        .iter()
        .map(|(study_id, _)| *study_id)
        .collect::<BTreeSet<_>>();
    if study_ids.len() != 1 {
        return Err(ambiguity_message("active studies", study_ids, "--study-id"));
    }
    let study_id = *study_ids.first().expect("one matching study");
    let selected_index = indexes
        .iter()
        .find(|(candidate, _)| *candidate == study_id)
        .map(|(_, index)| index)
        .expect("matching row came from an index");
    let all_study_entries = index_entries(selected_index, study_id)?;
    let candidate_offering_ids = matching
        .iter()
        .map(|(_, row)| {
            row.get("aktualni_predmet_id")
                .and_then(Value::as_u64)
                .ok_or_else(|| "invalid VUT subject identity".to_owned())
        })
        .collect::<Result<BTreeSet<_>, _>>()?;
    let mut offerings = BTreeMap::<u64, Vec<Value>>::new();
    for row in all_study_entries {
        let Some(offering_id) = row.get("aktualni_predmet_id").and_then(Value::as_u64) else {
            continue;
        };
        if candidate_offering_ids.contains(&offering_id) {
            offerings.entry(offering_id).or_default().push(row);
        }
    }
    for (offering_id, records) in &offerings {
        let first = records
            .first()
            .ok_or_else(|| "invalid VUT subject identity".to_owned())?;
        let expected =
            identity_from_record(first, study_id, *offering_id).map_err(str::to_owned)?;
        for record in &records[1..] {
            if identity_from_record(record, study_id, *offering_id).map_err(str::to_owned)?
                != expected
            {
                return Err("conflicting VUT subject identity".into());
            }
        }
    }

    let offering_id = if offerings.len() == 1 {
        *offerings.first_key_value().expect("one offering").0
    } else {
        let mut current = Vec::new();
        let mut starts = Vec::new();
        for (offering_id, records) in &offerings {
            let (start, end) = offering_interval(records)?;
            if start.as_str() <= today && today <= end.as_str() {
                current.push(*offering_id);
            }
            starts.push((*offering_id, start));
        }
        if current.len() == 1 {
            current[0]
        } else if current.len() > 1 {
            return Err(ambiguity_message(
                "current offerings",
                current,
                "--offering-id",
            ));
        } else {
            starts.sort_by(|left, right| right.1.cmp(&left.1));
            if starts.get(1).is_some_and(|next| next.1 == starts[0].1) {
                let tied = starts
                    .iter()
                    .take_while(|candidate| candidate.1 == starts[0].1)
                    .map(|candidate| candidate.0);
                return Err(ambiguity_message("offerings", tied, "--offering-id"));
            }
            starts[0].0
        }
    };
    let records = offerings
        .remove(&offering_id)
        .expect("selected offering exists");
    let mut identity = None;
    for record in &records {
        let current = identity_from_record(record, study_id, offering_id).map_err(str::to_owned)?;
        if identity.as_ref().is_some_and(|saved| saved != &current) {
            return Err("conflicting VUT subject identity".into());
        }
        identity = Some(current);
    }
    Ok(ResolvedLookup {
        identity: identity.ok_or_else(|| "invalid VUT subject identity".to_owned())?,
        records,
    })
}

pub(super) fn active_study_ids(studies: &Value) -> Result<Vec<u64>, String> {
    let rows = studies
        .pointer("/data/studia")
        .and_then(Value::as_array)
        .ok_or_else(|| "invalid VUT studies response".to_owned())?;
    let mut ids = BTreeSet::new();
    for row in rows {
        let active = row
            .get("aktivni_studium")
            .and_then(Value::as_u64)
            .filter(|value| *value <= 1)
            .ok_or_else(|| "invalid VUT active-study marker".to_owned())?;
        let study_id = row
            .get("studium_id")
            .and_then(Value::as_u64)
            .ok_or_else(|| "invalid VUT studies response".to_owned())?;
        if active == 1 {
            ids.insert(study_id);
        }
    }
    Ok(ids.into_iter().collect())
}

pub(super) fn resolve_active_indexes(
    studies: &Value,
    query: Option<&str>,
    offering_id: Option<u64>,
    today: &str,
    mut fetch_index: impl FnMut(u64) -> Result<Value, &'static str>,
) -> Result<ResolvedLookup, String> {
    let active_ids = active_study_ids(studies)?;
    if active_ids.is_empty() {
        return Err("no active study is available; use --study-id for historical access".into());
    }
    let mut indexes = Vec::new();
    let mut failed = Vec::new();
    for study_id in active_ids {
        match fetch_index(study_id) {
            Ok(index) => indexes.push((study_id, index)),
            Err(error)
                if matches!(
                    error,
                    "VUT API authentication rejected"
                        | "VUT API rate limited"
                        | "VUT authentication rate limited"
                ) =>
            {
                return Err(format!(
                    "could not read active study index {study_id}: {error}"
                ));
            }
            Err(_) => failed.push(study_id),
        }
    }
    if !failed.is_empty() {
        return Err(format!(
            "could not read every active study index (failed: {}); no subject was selected",
            failed
                .iter()
                .map(u64::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    resolve_lookup_in_indexes(&indexes, query, offering_id, today)
}

pub(super) fn resolve_subject(
    request: &SubjectRequest,
    today: &str,
) -> Result<ResolvedLookup, String> {
    if let Some(study_id) = request.study_id {
        let index = studies::fetch_index(study_id).map_err(str::to_owned)?;
        return resolve_lookup_in_indexes(
            &[(study_id, index)],
            request.code_or_name.as_deref(),
            request.offering_id,
            today,
        )
        .map_err(|error| {
            if error.starts_with("subject was not found") {
                format!("subject was not found in --study-id {study_id}")
            } else {
                error
            }
        });
    }
    let studies = studies::fetch().map_err(str::to_owned)?;
    resolve_active_indexes(
        &studies,
        request.code_or_name.as_deref(),
        request.offering_id,
        today,
        studies::fetch_index,
    )
}

pub(super) fn scope_from_request(
    request: &SubjectRequest,
    resolved: &ResolvedLookup,
) -> Result<RequestScope, String> {
    let explicit_route = request.code_or_name.is_none()
        && request.study_id.is_some()
        && request.offering_id.is_some();
    let (default_from, default_to) = if explicit_route {
        (None, None)
    } else {
        let semester_start = semester_date(
            resolved
                .records
                .first()
                .ok_or_else(|| "invalid VUT subject identity".to_owned())?,
            "zacatek_semestru",
        )?;
        let year = semester_start[0..4]
            .parse::<u64>()
            .map_err(|_| "invalid VUT subject semester interval".to_owned())?;
        let month = semester_start[5..7]
            .parse::<u64>()
            .map_err(|_| "invalid VUT subject semester interval".to_owned())?;
        let academic_year = if month >= 9 { year } else { year - 1 };
        (
            Some(format!("{academic_year:04}-09-01T00:00")),
            Some(format!("{:04}-08-31T23:59", academic_year + 1)),
        )
    };
    let from = request
        .from
        .clone()
        .or(default_from)
        .ok_or_else(|| "the explicit ID route requires --from".to_owned())?;
    let to = request
        .to
        .clone()
        .or(default_to)
        .ok_or_else(|| "the explicit ID route requires --to".to_owned())?;
    crate::dates::ordered(&from, &to).map_err(str::to_owned)?;
    let news_since = request
        .news_since
        .clone()
        .or_else(|| (!explicit_route).then(|| "1900-01-01".to_owned()))
        .ok_or_else(|| "the explicit ID route requires --news-since".to_owned())?;
    Ok(RequestScope {
        offering_id: resolved.identity.offering_id,
        study_id: resolved.identity.study_id,
        from,
        to,
        news_since,
        max_news: request.max_news,
    })
}

pub(super) fn vut_today_at(instant: SystemTime) -> String {
    let utc: chrono::DateTime<chrono::Utc> = instant.into();
    utc.with_timezone(&chrono_tz::Europe::Prague)
        .date_naive()
        .to_string()
}

pub(super) fn vut_today() -> String {
    vut_today_at(SystemTime::now())
}
