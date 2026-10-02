//! Bounded listing of direct Moodle resource activities for one resolved subject.

use serde::Serialize;

use crate::{
    resources::subjects,
    subject_view::{SubjectIdentity, SubjectRequest, resolve_identity},
    web::{
        moodle::{self, DirectResourceStatus, ResourceFile},
        studis,
    },
    web_session::WebSession,
};

const DIRECT_RESOURCE_LIMITATION: &str = "direct_resource_activities_only";
const MALFORMED_RESOURCE_LIMITATION: &str = "malformed_resource_activities_skipped";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum FileListStatus {
    Available,
    Empty,
    Unavailable,
}

#[derive(Serialize)]
pub(crate) struct MoodleFileList {
    schema_version: u8,
    subject: SubjectIdentity,
    status: FileListStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    course_url: Option<String>,
    files: Vec<ResourceFile>,
    limitations: Vec<&'static str>,
}

#[derive(Debug)]
pub(crate) struct DownloadSource {
    pub(crate) course_url: String,
    pub(crate) activity_url: String,
}

impl MoodleFileList {
    fn unavailable(
        subject: SubjectIdentity,
        reason: &'static str,
        course_url: Option<String>,
    ) -> Self {
        Self {
            schema_version: 1,
            subject,
            status: FileListStatus::Unavailable,
            reason: Some(reason),
            course_url,
            files: Vec::new(),
            limitations: vec![DIRECT_RESOURCE_LIMITATION],
        }
    }
}

fn api_error_reason(error: &'static str) -> &'static str {
    match error {
        "VUT API credentials are missing"
        | "VUT API authentication rejected"
        | "VUT API access token is empty" => "auth_required",
        "VUT API access denied" => "permission_denied",
        "VUT API rate limited" | "VUT authentication rate limited" => "rate_limited",
        error if error.starts_with("invalid VUT ") => "invalid_response",
        _ => "fetch_failed",
    }
}

fn page_error_reason(html: &str) -> &'static str {
    match moodle::page_error(html) {
        Some(moodle::PageError::AuthRequired) => "auth_required",
        Some(moodle::PageError::PermissionDenied) => "permission_denied",
        None => "invalid_web_page",
    }
}

fn list_from_course_page(
    subject: SubjectIdentity,
    course_url: String,
    html: &str,
) -> MoodleFileList {
    if page_error_reason(html) == "permission_denied" {
        return MoodleFileList::unavailable(subject, "permission_denied", Some(course_url));
    }
    let overview = match moodle::parse_overview(html, &course_url) {
        Ok(overview) => overview,
        Err(_) => {
            return MoodleFileList::unavailable(subject, page_error_reason(html), Some(course_url));
        }
    };
    let resources = overview.into_direct_resources();
    let (status, reason) = match resources.status {
        DirectResourceStatus::Available => (FileListStatus::Available, None),
        DirectResourceStatus::Empty => (FileListStatus::Empty, None),
        DirectResourceStatus::Unsupported => (FileListStatus::Unavailable, Some("unsupported")),
        DirectResourceStatus::Malformed => (FileListStatus::Unavailable, Some("invalid_web_page")),
    };
    let mut limitations = vec![DIRECT_RESOURCE_LIMITATION];
    if resources.malformed_resource_activities_skipped {
        limitations.push(MALFORMED_RESOURCE_LIMITATION);
    }
    MoodleFileList {
        schema_version: 1,
        subject,
        status,
        reason,
        course_url: Some(course_url),
        files: resources.files,
        limitations,
    }
}

fn course_url_from_api(raw: &serde_json::Value) -> Result<Option<String>, &'static str> {
    match subjects::verified_moodle_course_url(raw) {
        subjects::MoodleCourseLink::Missing => Ok(None),
        subjects::MoodleCourseLink::Unique(url) => Ok(Some(url)),
        subjects::MoodleCourseLink::Ambiguous => Err("ambiguous_moodle_links"),
    }
}

pub(crate) fn fetch(request: SubjectRequest) -> Result<MoodleFileList, String> {
    let subject = resolve_identity(&request)?;
    let raw = match subjects::fetch_moodle(subject.offering_id) {
        Ok(raw) => raw,
        Err(error) => {
            return Ok(MoodleFileList::unavailable(
                subject,
                api_error_reason(error),
                None,
            ));
        }
    };
    let api_course_url = match course_url_from_api(&raw) {
        Ok(url) => url,
        Err(reason) => return Ok(MoodleFileList::unavailable(subject, reason, None)),
    };
    let session = match WebSession::open(false, false) {
        Ok(session) => session,
        Err(error) => {
            return Ok(MoodleFileList::unavailable(
                subject,
                error.reason(),
                api_course_url,
            ));
        }
    };
    let course_url = match api_course_url {
        Some(url) => url,
        None => {
            let catalogue_url = format!(
                "https://www.vut.cz/studis/student.phtml?gm=gm_detail_predmetu&apid={}",
                subject.offering_id
            );
            let catalogue_html = match session.read(&catalogue_url) {
                Ok(html) => html,
                Err(error) => {
                    return Ok(MoodleFileList::unavailable(subject, error.reason(), None));
                }
            };
            let catalogue = match studis::parse_catalogue(&catalogue_html, &catalogue_url) {
                Ok(catalogue) => catalogue,
                Err(_) => {
                    return Ok(MoodleFileList::unavailable(
                        subject,
                        "invalid_web_page",
                        None,
                    ));
                }
            };
            let Some(url) = studis::moodle_course_url(&catalogue) else {
                return Ok(MoodleFileList::unavailable(
                    subject,
                    "no_verified_moodle_link",
                    None,
                ));
            };
            url
        }
    };
    let html = match session.read(&course_url) {
        Ok(html) => html,
        Err(error) => {
            return Ok(MoodleFileList::unavailable(
                subject,
                error.reason(),
                Some(course_url),
            ));
        }
    };
    Ok(list_from_course_page(subject, course_url, &html))
}

pub(crate) fn fetch_resource(
    request: SubjectRequest,
    module_id: u64,
) -> Result<DownloadSource, String> {
    resource_from_listing(fetch(request)?, module_id)
}

fn resource_from_listing(
    listing: MoodleFileList,
    module_id: u64,
) -> Result<DownloadSource, String> {
    if listing.status == FileListStatus::Unavailable {
        return Err(match listing.reason {
            Some("auth_required") => "VUT web login is required",
            Some("permission_denied") => "Moodle course access denied",
            Some("rate_limited") => "VUT rate limited the request",
            Some("no_verified_moodle_link") => "no verified Moodle course link is available",
            _ => "Moodle resource listing is unavailable",
        }
        .into());
    }
    let course_url = listing
        .course_url
        .ok_or_else(|| "Moodle resource listing is unavailable".to_owned())?;
    let file = listing
        .files
        .into_iter()
        .find(|file| file.module_id == module_id)
        .ok_or_else(|| "Moodle resource module was not found".to_owned())?;
    Ok(DownloadSource {
        course_url,
        activity_url: file.activity_url,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn subject() -> SubjectIdentity {
        SubjectIdentity {
            offering_id: 42,
            subject_id: 142,
            faculty_id: 13,
            academic_year: 2026,
            semester_type_id: 2,
            study_id: 7,
        }
    }

    #[test]
    fn direct_resources_have_a_stable_cli_owned_listing_contract() {
        let html = r#"<body id='page-course-view-topics'><ul><li class='section' data-sectionid='10'><h3 class='sectionname'>Lectures</h3><ul><li class='activity modtype_resource' data-id='5'><div class='activity-item' data-activityname='Slides'><a href='/mod/resource/view.php?id=5'>Slides</a></div></li></ul></li></ul></body>"#;
        let output = serde_json::to_value(list_from_course_page(
            subject(),
            "https://moodle.vut.cz/course/view.php?id=9".into(),
            html,
        ))
        .unwrap();

        assert_eq!(
            output,
            json!({
                "schema_version":1,
                "subject":{"offering_id":42,"subject_id":142,"faculty_id":13,"academic_year":2026,"semester_type_id":2,"study_id":7},
                "status":"available",
                "course_url":"https://moodle.vut.cz/course/view.php?id=9",
                "files":[{"module_id":5,"name":"Slides","section_id":10,"section_title":"Lectures","activity_url":"https://moodle.vut.cz/mod/resource/view.php?id=5"}],
                "limitations":["direct_resource_activities_only"]
            })
        );
    }

    #[test]
    fn unsupported_activities_and_denied_pages_are_not_reported_as_empty() {
        let unsupported = r#"<body id='page-course-view-topics'><ul><li class='section' data-sectionid='10'><ul><li class='activity modtype_folder' data-id='6'><div class='activity-item' data-activityname='Folder'><a href='/mod/folder/view.php?id=6'>Folder</a></div></li></ul></li></ul></body>"#;
        let unsupported = serde_json::to_value(list_from_course_page(
            subject(),
            "https://moodle.vut.cz/course/view.php?id=9".into(),
            unsupported,
        ))
        .unwrap();
        assert_eq!(unsupported["status"], "unavailable");
        assert_eq!(unsupported["reason"], "unsupported");

        let malformed_resource = r#"<body id='page-course-view-topics'><ul><li class='section' data-sectionid='10'><ul><li class='activity modtype_resource'><div class='activity-item' data-activityname='Broken'><a href='/mod/resource/view.php'>Broken</a></div></li></ul></li></ul></body>"#;
        let malformed_resource = serde_json::to_value(list_from_course_page(
            subject(),
            "https://moodle.vut.cz/course/view.php?id=9".into(),
            malformed_resource,
        ))
        .unwrap();
        assert_eq!(malformed_resource["status"], "unavailable");
        assert_eq!(malformed_resource["reason"], "invalid_web_page");

        let zero_module = r#"<body id='page-course-view-topics'><ul><li class='section' data-sectionid='10'><ul><li class='activity modtype_resource' data-id='0'><div class='activity-item' data-activityname='Broken'><a href='/mod/resource/view.php?id=0'>Broken</a></div></li></ul></li></ul></body>"#;
        let zero_module = serde_json::to_value(list_from_course_page(
            subject(),
            "https://moodle.vut.cz/course/view.php?id=9".into(),
            zero_module,
        ))
        .unwrap();
        assert_eq!(zero_module["status"], "unavailable");
        assert_eq!(zero_module["reason"], "invalid_web_page");
        assert!(zero_module["files"].as_array().unwrap().is_empty());

        let denied = serde_json::to_value(list_from_course_page(
            subject(),
            "https://moodle.vut.cz/course/view.php?id=9".into(),
            "<body id='page-course-view-topics'><div class='alert alert-danger mb-3'>Access denied</div></body>",
        ))
        .unwrap();
        assert_eq!(denied["status"], "unavailable");
        assert_eq!(denied["reason"], "permission_denied");
    }

    #[test]
    fn empty_and_malformed_resource_precedence_are_stable() {
        let empty = serde_json::to_value(list_from_course_page(
            subject(),
            "https://moodle.vut.cz/course/view.php?id=9".into(),
            "<body id='page-course-view-topics'><ul><li class='section' data-sectionid='10'></li></ul></body>",
        ))
        .unwrap();
        assert_eq!(empty["status"], "empty");
        assert!(empty.get("reason").is_none());

        let mixed = r#"<body id='page-course-view-topics'><ul><li class='section' data-sectionid='10'><ul><li class='activity modtype_resource' data-id='5'><div class='activity-item' data-activityname='Slides'><a href='/mod/resource/view.php?id=5'>Slides</a></div></li><li class='activity modtype_folder' data-id='6'><div class='activity-item' data-activityname='Folder'><a href='/mod/folder/view.php?id=6'>Folder</a></div></li><li class='activity modtype_resource'><div class='activity-item' data-activityname='Broken'><a href='/mod/resource/view.php'>Broken</a></div></li></ul></li></ul></body>"#;
        let mixed = serde_json::to_value(list_from_course_page(
            subject(),
            "https://moodle.vut.cz/course/view.php?id=9".into(),
            mixed,
        ))
        .unwrap();
        assert_eq!(mixed["status"], "available");
        assert!(mixed.get("reason").is_none());
        assert_eq!(mixed["files"].as_array().unwrap().len(), 1);
        assert_eq!(
            mixed["limitations"],
            json!([
                "direct_resource_activities_only",
                "malformed_resource_activities_skipped"
            ])
        );

        let listing = list_from_course_page(
            subject(),
            "https://moodle.vut.cz/course/view.php?id=9".into(),
            r#"<body id='page-course-view-topics'><ul><li class='section' data-sectionid='10'><ul><li class='activity modtype_resource' data-id='5'><div class='activity-item' data-activityname='Slides'><a href='/mod/resource/view.php?id=5'>Slides</a></div></li><li class='activity modtype_resource'><div class='activity-item' data-activityname='Broken'><a href='/mod/resource/view.php'>Broken</a></div></li></ul></li></ul></body>"#,
        );
        assert_eq!(
            resource_from_listing(listing, 5).unwrap().activity_url,
            "https://moodle.vut.cz/mod/resource/view.php?id=5"
        );
    }

    #[test]
    fn download_selection_accepts_only_a_listed_resource_module() {
        let html = r#"<body id='page-course-view-topics'><ul><li class='section' data-sectionid='10'><h3 class='sectionname'>Lectures</h3><ul><li class='activity modtype_resource' data-id='5'><div class='activity-item' data-activityname='Slides'><a href='/mod/resource/view.php?id=5'>Slides</a></div></li></ul></li></ul></body>"#;
        let listing = list_from_course_page(
            subject(),
            "https://moodle.vut.cz/course/view.php?id=9".into(),
            html,
        );

        let selected = resource_from_listing(listing, 5).unwrap();
        assert_eq!(
            selected.course_url,
            "https://moodle.vut.cz/course/view.php?id=9"
        );
        assert_eq!(
            selected.activity_url,
            "https://moodle.vut.cz/mod/resource/view.php?id=5"
        );

        let listing = list_from_course_page(
            subject(),
            "https://moodle.vut.cz/course/view.php?id=9".into(),
            html,
        );
        assert_eq!(
            resource_from_listing(listing, 6).unwrap_err(),
            "Moodle resource module was not found"
        );
    }

    #[test]
    fn ambiguous_api_course_links_are_unavailable_without_fallback() {
        let raw = json!({"data":{"odkazy":[
            {"odkaz_moodle":"https://moodle.vut.cz/course/view.php?id=42"},
            {"odkaz_moodle":"https://moodle.vut.cz/course/view.php?id=43"}
        ]}});
        let reason = course_url_from_api(&raw).unwrap_err();
        let listing = MoodleFileList::unavailable(subject(), reason, None);
        let output = serde_json::to_value(listing).unwrap();
        assert_eq!(output["status"], "unavailable");
        assert_eq!(output["reason"], "ambiguous_moodle_links");
        assert!(output.get("course_url").is_none());
        assert!(output["files"].as_array().unwrap().is_empty());
    }
}
