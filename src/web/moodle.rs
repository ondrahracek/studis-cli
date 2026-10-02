//! Moodle course overview extraction from its authenticated HTML page.

use scraper::{Html, Selector};
use serde::Serialize;

use crate::moodle_url;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct CourseOverview {
    sections: Vec<CourseSection>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct CourseSection {
    id: Option<u64>,
    title: String,
    activities: Vec<CourseActivity>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct CourseActivity {
    id: Option<u64>,
    #[serde(rename = "type")]
    kind: String,
    name: String,
    url: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct ResourceFile {
    pub(crate) module_id: u64,
    pub(crate) name: String,
    pub(crate) section_id: Option<u64>,
    pub(crate) section_title: String,
    pub(crate) activity_url: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DirectResourceStatus {
    Available,
    Empty,
    Unsupported,
    Malformed,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct DirectResources {
    pub(crate) status: DirectResourceStatus,
    pub(crate) files: Vec<ResourceFile>,
    pub(crate) malformed_resource_activities_skipped: bool,
}

impl CourseOverview {
    pub(crate) fn into_sections(self) -> Vec<CourseSection> {
        self.sections
    }

    pub(crate) fn into_direct_resources(self) -> DirectResources {
        let mut files = Vec::new();
        let mut malformed_resource = false;
        let mut unsupported_activity = false;

        for section in self.sections {
            for activity in section.activities {
                if activity.kind != "resource" {
                    unsupported_activity = true;
                    continue;
                }
                let (Some(module_id), Some(activity_url)) = (activity.id, activity.url) else {
                    malformed_resource = true;
                    continue;
                };
                files.push(ResourceFile {
                    module_id,
                    name: activity.name,
                    section_id: section.id,
                    section_title: section.title.clone(),
                    activity_url,
                });
            }
        }

        let status = if !files.is_empty() {
            DirectResourceStatus::Available
        } else if malformed_resource {
            DirectResourceStatus::Malformed
        } else if unsupported_activity {
            DirectResourceStatus::Unsupported
        } else {
            DirectResourceStatus::Empty
        };
        DirectResources {
            status,
            files,
            malformed_resource_activities_skipped: malformed_resource,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PageError {
    AuthRequired,
    PermissionDenied,
}

fn selector(css: &str) -> Selector {
    Selector::parse(css).expect("fixed Moodle CSS selector")
}

pub(crate) fn page_error(html: &str) -> Option<PageError> {
    let document = Html::parse_document(html);
    let body = document.select(&selector("body[id^='page-']")).next()?;
    if body.value().id() == Some("page-login-index")
        || body
            .select(&selector("form[action]"))
            .filter_map(|form| form.value().attr("action"))
            .any(|action| action.contains("/login/index.php"))
    {
        return Some(PageError::AuthRequired);
    }
    body.select(&selector(".errorbox,.alert-danger,[data-rel='fatalerror']"))
        .next()
        .map(|_| PageError::PermissionDenied)
}

pub(crate) fn parse_overview(html: &str, base: &str) -> Result<CourseOverview, &'static str> {
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
        for activity in section.select(&selector("li.activity")) {
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
                .find_map(|url| moodle_url::canonical_activity_url(&url, kind, id?));
            activities.push(CourseActivity {
                id,
                kind: kind.to_owned(),
                name: name.to_owned(),
                url,
            });
        }
        sections.push(CourseSection {
            id,
            title,
            activities,
        });
    }
    Ok(CourseOverview { sections })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn overview_extracts_sections_and_activity_links_without_opening_them() {
        let html = r#"<body id='page-course-view-topics'><ul><li class='section' id='section-0' data-sectionid='0'><h3 class='sectionname'>Lectures</h3><ul><li class='activity modtype_resource' id='module-5' data-id='5'><a href='/local/noise.php'>Status</a><div class='activity-item' data-activityname='Slides'><a href='/mod/resource/view.php?id=5'>Slides</a></div></li></ul></li></ul></body>"#;
        let parsed = serde_json::to_value(
            parse_overview(html, "https://moodle.vut.cz/course/view.php?id=42").unwrap(),
        )
        .unwrap();
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

    #[test]
    fn resource_files_preserve_section_context_and_skip_other_activity_types() {
        let html = r#"<body id='page-course-view-topics'><ul>
            <li class='section' data-sectionid='10'><h3 class='sectionname'>Lectures</h3><ul>
                <li class='activity modtype_resource' data-id='5'><div class='activity-item' data-activityname='Slides'><a href='/mod/resource/view.php?id=5'>Slides</a></div></li>
                <li class='activity modtype_quiz' data-id='6'><div class='activity-item' data-activityname='Quiz'><a href='/mod/quiz/view.php?id=6'>Quiz</a></div></li>
                <li class='activity modtype_resource'><div class='activity-item' data-activityname='Broken'><a href='/mod/resource/view.php'>Broken</a></div></li>
                <li class='activity modtype_resource' data-id='8'><div class='activity-item' data-activityname='Wrong port'><a href='https://moodle.vut.cz:444/mod/resource/view.php?id=8'>Wrong port</a></div></li>
            </ul></li>
            <li class='section' data-sectionid='11'><h3 class='sectionname'>Labs</h3><ul>
                <li class='activity modtype_resource' data-id='7'><div class='activity-item' data-activityname='Assignment'><a href='/mod/resource/view.php?id=7'>Assignment</a></div></li>
            </ul></li>
        </ul></body>"#;
        let overview = parse_overview(html, "https://moodle.vut.cz/course/view.php?id=42").unwrap();

        assert_eq!(
            overview.into_direct_resources(),
            DirectResources {
                status: DirectResourceStatus::Available,
                files: vec![
                    ResourceFile {
                        module_id: 5,
                        name: "Slides".into(),
                        section_id: Some(10),
                        section_title: "Lectures".into(),
                        activity_url: "https://moodle.vut.cz/mod/resource/view.php?id=5".into(),
                    },
                    ResourceFile {
                        module_id: 7,
                        name: "Assignment".into(),
                        section_id: Some(11),
                        section_title: "Labs".into(),
                        activity_url: "https://moodle.vut.cz/mod/resource/view.php?id=7".into(),
                    },
                ],
                malformed_resource_activities_skipped: true,
            }
        );
    }

    #[test]
    fn activity_links_reject_extra_query_data_and_are_canonicalized() {
        let unsafe_html = r#"<body id='page-course-view-topics'><ul><li class='section' data-sectionid='1'><ul><li class='activity modtype_resource' data-id='5'><div class='activity-item' data-activityname='Unsafe'><a href='/mod/resource/view.php?id=5&amp;token=secret'>Unsafe</a></div></li></ul></li></ul></body>"#;
        let unsafe_overview = serde_json::to_value(
            parse_overview(unsafe_html, "https://moodle.vut.cz/course/view.php?id=42").unwrap(),
        )
        .unwrap();
        assert!(
            unsafe_overview["sections"][0]["activities"][0]["url"].is_null(),
            "extra query data reached the CLI-owned listing"
        );

        let canonical_html = r#"<body id='page-course-view-topics'><ul><li class='section' data-sectionid='1'><ul><li class='activity modtype_resource' data-id='5'><div class='activity-item' data-activityname='Safe'><a href='/mod/resource/view.php?id=0005'>Safe</a></div></li></ul></li></ul></body>"#;
        let canonical_overview = serde_json::to_value(
            parse_overview(
                canonical_html,
                "https://moodle.vut.cz/course/view.php?id=42",
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(
            canonical_overview["sections"][0]["activities"][0]["url"],
            "https://moodle.vut.cz/mod/resource/view.php?id=5"
        );
    }
}
