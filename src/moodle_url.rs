//! Canonical Moodle URLs and the fixed trusted origin.

use reqwest::Url;

pub(crate) const HOST: &str = "moodle.vut.cz";
const ORIGIN: &str = "https://moodle.vut.cz";

pub(crate) fn has_exact_origin(url: &Url) -> bool {
    url.scheme() == "https"
        && url.host_str() == Some(HOST)
        && url.port().is_none()
        && url.username().is_empty()
        && url.password().is_none()
        && url.fragment().is_none()
}

pub(crate) fn canonical_course_url(raw: &str) -> Option<String> {
    let url = Url::parse(raw).ok()?;
    let id = single_id(&url, "/course/view.php")?;
    (id > 0).then(|| format!("{ORIGIN}/course/view.php?id={id}"))
}

pub(crate) fn canonical_activity_url(url: &Url, kind: &str, module_id: u64) -> Option<String> {
    let path = format!("/mod/{kind}/view.php");
    (module_id > 0 && single_id(url, &path)? == module_id)
        .then(|| format!("{ORIGIN}{path}?id={module_id}"))
}

pub(crate) fn is_payload_url(url: &Url) -> bool {
    has_exact_origin(url) && url.path().starts_with("/pluginfile.php/")
}

fn single_id(url: &Url, path: &str) -> Option<u64> {
    if !has_exact_origin(url) || url.path() != path {
        return None;
    }
    let mut query = url.query_pairs();
    let (key, value) = query.next()?;
    if key != "id" || query.next().is_some() {
        return None;
    }
    value.parse().ok()
}
