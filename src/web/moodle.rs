//! Moodle course overview extraction from its authenticated HTML page.

use scraper::{Html, Selector};
use serde_json::{Value, json};

fn selector(css: &str) -> Selector {
    Selector::parse(css).expect("fixed Moodle CSS selector")
}

pub(crate) fn parse_overview(html: &str, base: &str) -> Result<Value, &'static str> {
    let document = Html::parse_document(html);
    let body = document
        .select(&selector("body[id^='page-course-view']"))
        .next()
        .ok_or("invalid Moodle course page")?;
    let mut sections = Vec::new();
    for section in body.select(&selector("li.section[data-sectionid]")) {
        let id = section
            .value()
            .attr("data-sectionid")
            .and_then(|value| value.parse::<u64>().ok());
        let title = section
            .select(&selector(".sectionname,.section-title,h2,h3"))
            .next()
            .map(|element| {
                element
                    .text()
                    .collect::<Vec<_>>()
                    .join(" ")
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .unwrap_or_default();
        let mut activities = Vec::new();
        for activity in section.select(&selector("li.activity[data-id]")) {
            let id = activity
                .value()
                .attr("data-id")
                .and_then(|value| value.parse::<u64>().ok());
            let kind = activity
                .value()
                .classes()
                .find_map(|class| class.strip_prefix("modtype_"))
                .unwrap_or("unknown");
            let name = activity
                .select(&selector(".activity-item[data-activityname]"))
                .next()
                .and_then(|item| item.value().attr("data-activityname"))
                .unwrap_or("");
            let url = activity
                .select(&selector("a[href]"))
                .filter_map(|anchor| anchor.value().attr("href"))
                .filter_map(|href| reqwest::Url::parse(base).ok()?.join(href).ok())
                .find(|url| url.scheme() == "https" && url.host_str() == Some("moodle.vut.cz"))
                .map(|url| url.to_string());
            activities.push(json!({"id":id,"type":kind,"name":name,"url":url}));
        }
        sections.push(json!({"id":id,"title":title,"activities":activities}));
    }
    Ok(json!({"sections":sections}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn overview_extracts_sections_and_activity_links_without_opening_them() {
        let html = r#"<body id='page-course-view-topics'><ul><li class='section' id='section-0' data-sectionid='0'><h3 class='sectionname'>Lectures</h3><ul><li class='activity modtype_resource' id='module-5' data-id='5'><div class='activity-item' data-activityname='Slides'><a href='/mod/resource/view.php?id=5'>Slides</a></div></li></ul></li></ul></body>"#;
        let parsed = parse_overview(html, "https://moodle.vut.cz/course/view.php?id=42").unwrap();
        assert_eq!(parsed["sections"][0]["title"], "Lectures");
        assert_eq!(
            parsed["sections"][0]["activities"][0],
            json!({"id":5,"type":"resource","name":"Slides","url":"https://moodle.vut.cz/mod/resource/view.php?id=5"})
        );
        assert!(
            parse_overview(
                "<body>Login</body>",
                "https://moodle.vut.cz/course/view.php?id=42"
            )
            .is_err()
        );
    }
}
