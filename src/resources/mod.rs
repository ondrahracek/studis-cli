//! Typed operations over documented VUT resources.

pub(crate) fn parse_required_array(
    body: &str,
    key: &str,
    error: &'static str,
) -> Result<serde_json::Value, &'static str> {
    let raw: serde_json::Value = serde_json::from_str(body).map_err(|_| error)?;
    if !raw
        .get("data")
        .and_then(|data| data.get(key))
        .is_some_and(serde_json::Value::is_array)
    {
        return Err(error);
    }
    Ok(raw)
}

pub(crate) fn parse_list(body: &str, key: &str) -> Result<serde_json::Value, &'static str> {
    let raw: serde_json::Value =
        serde_json::from_str(body).map_err(|_| "invalid VUT list response")?;
    let data = raw
        .get("data")
        .and_then(serde_json::Value::as_object)
        .ok_or("invalid VUT list response")?;
    if !data.is_empty() && !data.get(key).is_some_and(serde_json::Value::is_array) {
        return Err("invalid VUT list response");
    }
    Ok(raw)
}

pub(crate) mod account;
pub(crate) mod news;
pub(crate) mod schedule;
pub(crate) mod studies;
pub(crate) mod subjects;
