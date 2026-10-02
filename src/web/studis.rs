//! Studis HTML extraction from fixed read-only pages.

use scraper::{ElementRef, Html, Selector};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

fn selector(css: &str) -> Selector {
    Selector::parse(css).expect("fixed CSS selector")
}

fn plain_text(element: ElementRef<'_>) -> String {
    let mut text = element
        .text()
        .collect::<Vec<_>>()
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(32_000)
        .collect::<String>();
    for mark in [".", ",", ":", ";", "!", "?"] {
        text = text.replace(&format!(" {mark}"), mark);
    }
    text
}

fn safe_url(base: &str, href: &str) -> Option<String> {
    let url = reqwest::Url::parse(base).ok()?.join(href).ok()?;
    (url.scheme() == "https"
        && url.username().is_empty()
        && url.password().is_none()
        && url.as_str().len() <= 2_048)
        .then(|| url.to_string())
}

fn links(element: ElementRef<'_>, base: &str) -> Vec<Value> {
    element
        .select(&selector("a[href]"))
        .filter_map(|link| {
            let url = safe_url(base, link.value().attr("href")?)?;
            Some(json!({"text":plain_text(link),"url":url}))
        })
        .collect()
}

pub(crate) fn parse_catalogue(html: &str, base: &str) -> Result<Value, &'static str> {
    let document = Html::parse_document(html);
    let main = document
        .select(&selector(".vut-main-content"))
        .next()
        .ok_or("invalid Studis catalogue page")?;
    let mut fields = Vec::new();
    for row in main.select(&selector("table.table-bordered tr, table.data tr")) {
        let Some(label) = row.select(&selector("th")).next() else {
            continue;
        };
        let Some(value) = row.select(&selector("td")).next() else {
            continue;
        };
        let label = plain_text(label).trim_end_matches(':').to_owned();
        if !label.is_empty() {
            fields.push(json!({"label":label,"text":plain_text(value),"links":links(value,base)}));
        }
    }
    if fields.is_empty() {
        return Err("invalid Studis catalogue page");
    }
    Ok(json!({"fields":fields}))
}

pub(crate) fn moodle_course_url(catalogue: &Value) -> Option<String> {
    fn names_moodle(value: &str) -> bool {
        value
            .split(|character: char| !character.is_alphanumeric())
            .any(|word| caseless::canonical_caseless_match_str(word, "moodle"))
    }

    let mut candidates = BTreeSet::new();
    for field in catalogue
        .get("fields")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let field_names_moodle = field
            .get("label")
            .and_then(Value::as_str)
            .is_some_and(names_moodle);
        for link in field
            .get("links")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let link_names_moodle = link
                .get("text")
                .and_then(Value::as_str)
                .is_some_and(names_moodle);
            if (field_names_moodle || link_names_moodle)
                && let Some(url) = link
                    .get("url")
                    .and_then(Value::as_str)
                    .and_then(crate::moodle_url::canonical_course_url)
            {
                candidates.insert(url);
            }
        }
    }
    (candidates.len() == 1).then(|| candidates.pop_first().expect("one Moodle URL"))
}

pub(crate) fn parse_personal_detail(html: &str) -> Result<Value, &'static str> {
    let document = Html::parse_document(html);
    let main = document
        .select(&selector(".vut-main-content"))
        .next()
        .ok_or("invalid Studis personal subject page")?;
    let mut fields = Vec::new();
    if let Some(dl) = main.select(&selector("dl.dl-horizontal")).next() {
        let label_selector = selector("dt");
        let value_selector = selector("dd");
        let labels = dl.select(&label_selector);
        let values = dl.select(&value_selector);
        for (label, value) in labels.zip(values) {
            let label = plain_text(label).trim_end_matches(':').to_owned();
            if !label.is_empty() {
                fields.push(json!({"label":label,"text":plain_text(value)}));
            }
        }
    }
    let mut assessments = Vec::new();
    if let Some(table) = main.select(&selector("table.table-bordered")).next() {
        let row_selector = selector("tr");
        let mut rows = table.select(&row_selector);
        if let Some(header) = rows.next() {
            let headers: Vec<_> = header.select(&selector("th")).map(plain_text).collect();
            for row in rows {
                let cells: Vec<_> = row.select(&selector("th,td")).map(plain_text).collect();
                if cells.len() != headers.len() {
                    continue;
                }
                let values: BTreeMap<_, _> = headers.iter().cloned().zip(cells).collect();
                assessments.push(json!(values));
            }
        }
    }
    if fields.is_empty() && assessments.is_empty() {
        return Err("invalid Studis personal subject page");
    }
    Ok(json!({"fields":fields,"assessments":assessments}))
}

fn iso_day(value: &str) -> Option<String> {
    let mut parts = value.split('.');
    let day = parts.next()?.parse::<u32>().ok()?;
    let month = parts.next()?.parse::<u32>().ok()?;
    let year = parts.next()?.parse::<u32>().ok()?;
    if parts.next().is_some() {
        return None;
    }
    let date = format!("{year:04}-{month:02}-{day:02}");
    crate::dates::date(&date).ok()
}

fn tooltip_details(html: &str) -> BTreeMap<String, String> {
    let fragment = Html::parse_fragment(html);
    let mut details = BTreeMap::new();
    for row in fragment.select(&selector("tr")) {
        if let (Some(label), Some(value)) = (
            row.select(&selector("th")).next(),
            row.select(&selector("td")).next(),
        ) {
            let label = plain_text(label).trim_end_matches(':').to_owned();
            if !label.is_empty() {
                details.insert(label, plain_text(value));
            }
        }
    }
    details
}

fn event_times(date: &str, interval: &str) -> Option<(String, String)> {
    let (start, end) = interval.split_once(['–', '—', '-'])?;
    let start = crate::dates::local_datetime(&format!("{date}T{}", start.trim())).ok()?;
    let end = crate::dates::local_datetime(&format!("{date}T{}", end.trim())).ok()?;
    (start <= end).then_some((start, end))
}

pub(crate) fn parse_timetable(html: &str, from: &str, to: &str) -> Result<Value, &'static str> {
    let document = Html::parse_document(html);
    let calendar = document
        .select(&selector(".rozvrh"))
        .next()
        .ok_or("invalid Studis timetable page")?;
    let from_date = from.get(..10).ok_or("invalid timetable window")?;
    let to_date = to.get(..10).ok_or("invalid timetable window")?;
    let mut dates = Vec::new();
    let mut events = Vec::new();
    let mut times_verified = true;
    for day in calendar.select(&selector(".den")) {
        let date = day
            .select(&selector(".popis[data-datumdne]"))
            .next()
            .and_then(|node| node.value().attr("data-datumdne"))
            .and_then(iso_day);
        let Some(date) = date else { continue };
        dates.push(date.clone());
        if date.as_str() < from_date || date.as_str() > to_date {
            continue;
        }
        for block in day.select(&selector(".blok:not(.blok-nic)")) {
            let Some(inner) = block.select(&selector(".blok-vn")).next() else {
                continue;
            };
            let title = inner.value().attr("data-bs-title").unwrap_or("");
            let details = inner
                .value()
                .attr("data-bs-content")
                .map(tooltip_details)
                .unwrap_or_default();
            let (starts_at, ends_at) = match details
                .get("Doba")
                .and_then(|interval| event_times(&date, interval))
            {
                Some((start, end)) => {
                    if end.as_str() < from || start.as_str() > to {
                        continue;
                    }
                    (Some(start), Some(end))
                }
                None => {
                    times_verified = false;
                    (None, None)
                }
            };
            let event_links = links(inner, "https://www.vut.cz/studis/student.phtml");
            events.push(json!({"date":date,"starts_at":starts_at,"ends_at":ends_at,"title":title,"text":plain_text(inner),"details":details,"links":event_links}));
        }
    }
    dates.sort();
    let (Some(first), Some(last)) = (dates.first(), dates.last()) else {
        return Err("invalid Studis timetable page");
    };
    Ok(
        json!({"covered_from":first,"covered_to":last,"window_covered":first.as_str()<=from_date && last.as_str()>=to_date,"times_verified":times_verified,"events":events}),
    )
}

pub(crate) fn parse_announcement_detail(html: &str, base: &str) -> Result<Value, &'static str> {
    let document = Html::parse_document(html);
    let main = document
        .select(&selector(".vut-main-content"))
        .next()
        .ok_or("invalid Studis announcement page")?;
    let title = main
        .select(&selector("h3"))
        .next()
        .map(plain_text)
        .ok_or("invalid Studis announcement page")?;
    let mut fields = Vec::new();
    if let Some(dl) = main.select(&selector("dl")).next() {
        let labels: Vec<_> = dl.select(&selector("dt")).map(plain_text).collect();
        let values: Vec<_> = dl.select(&selector("dd")).map(plain_text).collect();
        for (label, value) in labels.into_iter().zip(values) {
            fields.push(json!({"label":label.trim_end_matches(':'),"text":value}));
        }
    }
    let mut body_paragraphs = Vec::new();
    let mut body_links = Vec::new();
    for paragraph in main.select(&selector("p")) {
        if paragraph.select(&selector("a.btn")).next().is_some() {
            continue;
        }
        let text = plain_text(paragraph);
        if !text.is_empty() {
            body_paragraphs.push(text);
        }
        body_links.extend(links(paragraph, base));
    }
    Ok(json!({"title":title,"fields":fields,"body_paragraphs":body_paragraphs,"links":body_links}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn catalogue_extracts_field_text_and_safe_links_from_main_content() {
        let html = r#"<html><body><nav><a href='https://evil.example'>ignore</a></nav><div class='vut-main-content'><h1>Detail předmětu</h1><table class='data'><tr><th>Cíle předmětu:</th><td><p>Learn <b>systems</b>.</p><a href='https://moodle.vut.cz/course/view.php?id=42'>Moodle</a><a href='javascript:alert(1)'>bad</a></td></tr></table></div></body></html>"#;
        let parsed = parse_catalogue(
            html,
            "https://www.vut.cz/studis/student.phtml?gm=gm_detail_predmetu&apid=42",
        )
        .unwrap();
        assert_eq!(parsed["fields"][0]["label"], "Cíle předmětu");
        assert!(
            parsed["fields"][0]["text"]
                .as_str()
                .unwrap()
                .contains("Learn systems.")
        );
        assert_eq!(
            parsed["fields"][0]["links"],
            json!([{"text":"Moodle","url":"https://moodle.vut.cz/course/view.php?id=42"}])
        );
        assert_eq!(
            moodle_course_url(&parsed).as_deref(),
            Some("https://moodle.vut.cz/course/view.php?id=42")
        );
        assert_eq!(
            moodle_course_url(
                &json!({"fields":[{"links":[{"url":"https://moodle.vut.cz.evil.example/course/view.php?id=42"}]}]})
            ),
            None
        );
        assert_eq!(
            moodle_course_url(
                &json!({"fields":[{"label":"Literature","links":[{"text":"Course notes","url":"https://moodle.vut.cz/course/view.php?id=42"}]}]})
            ),
            None
        );
        assert_eq!(
            moodle_course_url(&json!({"fields":[
                {"label":"Moodle","links":[{"text":"Course","url":"https://moodle.vut.cz/course/view.php?id=42"}]},
                {"label":"Resources","links":[{"text":"Moodle","url":"https://moodle.vut.cz/course/view.php?id=43"}]}
            ]})),
            None
        );
        assert!(
            parse_catalogue(
                "<html><body>login</body></html>",
                "https://www.vut.cz/studis/student.phtml"
            )
            .is_err()
        );
    }

    #[test]
    fn personal_detail_extracts_summary_and_assessment_rows() {
        let html = r#"<div class='vut-main-content'><h1>Detail předmětu</h1><dl class='dl-horizontal'><dt>Kredity:</dt><dd>5</dd></dl><table class='table table-middle table-bordered'><tr><th>Název</th><th>Body</th></tr><tr><th>Test</th><td>8</td></tr></table></div>"#;
        let parsed = parse_personal_detail(html).unwrap();
        assert_eq!(parsed["fields"][0], json!({"label":"Kredity","text":"5"}));
        assert_eq!(parsed["assessments"][0]["Název"], "Test");
        assert_eq!(parsed["assessments"][0]["Body"], "8");
    }

    #[test]
    fn timetable_filters_to_requested_days_and_reports_coverage() {
        let html = r#"<div class='rozvrh'><div class='den'><div class='popis' data-datumdne='28.09.2026'></div><div class='blok'><div class='blok-vn' data-bs-title='Course' data-bs-content='&lt;table&gt;&lt;tr&gt;&lt;th&gt;Doba:&lt;/th&gt;&lt;td&gt;08:00–09:50&lt;/td&gt;&lt;/tr&gt;&lt;/table&gt;'></div></div></div><div class='den'><div class='popis' data-datumdne='29.09.2026'></div><div class='blok blok-nic'></div></div></div>"#;
        let parsed = parse_timetable(html, "2026-09-28T00:00", "2026-09-29T23:59").unwrap();
        assert_eq!(parsed["covered_from"], "2026-09-28");
        assert_eq!(parsed["covered_to"], "2026-09-29");
        assert_eq!(parsed["window_covered"], true);
        assert_eq!(parsed["events"].as_array().unwrap().len(), 1);
        assert_eq!(parsed["events"][0]["details"]["Doba"], "08:00–09:50");
    }

    #[test]
    fn timetable_filters_events_at_datetime_boundaries() {
        let html = r#"<div class='rozvrh'><div class='den'><div class='popis' data-datumdne='28.09.2026'></div><div class='blok'><div class='blok-vn' data-bs-content='&lt;table&gt;&lt;tr&gt;&lt;th&gt;Doba:&lt;/th&gt;&lt;td&gt;08:00–09:50&lt;/td&gt;&lt;/tr&gt;&lt;/table&gt;'></div></div><div class='blok'><div class='blok-vn' data-bs-content='&lt;table&gt;&lt;tr&gt;&lt;th&gt;Doba:&lt;/th&gt;&lt;td&gt;10:00–11:50&lt;/td&gt;&lt;/tr&gt;&lt;/table&gt;'></div></div></div></div>"#;
        let parsed = parse_timetable(html, "2026-09-28T08:30", "2026-09-28T09:00").unwrap();
        assert_eq!(parsed["events"].as_array().unwrap().len(), 1);
        assert_eq!(parsed["events"][0]["starts_at"], "2026-09-28T08:00");
        assert_eq!(parsed["events"][0]["ends_at"], "2026-09-28T09:50");
    }

    #[test]
    fn announcement_detail_extracts_body_and_link_without_back_button() {
        let html = r#"<div class='vut-main-content'><h1>News</h1><p><a class='btn' href='?sn=aktuality_predmet'>Back</a></p><h3>Update</h3><dl><dt>Platnost:</dt><dd>October</dd></dl><hr><p>Read <a href='https://example.org/info'>details</a>.</p></div>"#;
        let parsed = parse_announcement_detail(
            html,
            "https://www.vut.cz/studis/student.phtml?sn=aktuality_predmet&akce=2&did=5&apid=42",
        )
        .unwrap();
        assert_eq!(parsed["title"], "Update");
        assert_eq!(parsed["body_paragraphs"], json!(["Read details."]));
        assert_eq!(
            parsed["links"],
            json!([{"text":"details","url":"https://example.org/info"}])
        );
    }
}
