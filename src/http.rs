//! HTTP client settings shared by the token and studies requests.

use std::time::Duration;

use reqwest::blocking::{Client, Request};

pub(crate) fn client() -> Result<reqwest::blocking::Client, &'static str> {
    reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(20))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| "unable to initialize VUT HTTP client")
}

pub(crate) fn get_body(client: &Client, request: Request) -> Result<String, &'static str> {
    let response = client
        .execute(request)
        .map_err(|_| "unable to contact VUT API")?;
    get_status(response.status())?;
    response
        .text()
        .map_err(|_| "unable to read VUT API response")
}

fn get_status(status: reqwest::StatusCode) -> Result<(), &'static str> {
    match status {
        reqwest::StatusCode::OK => Ok(()),
        reqwest::StatusCode::FORBIDDEN => Err("VUT API access denied"),
        reqwest::StatusCode::TOO_MANY_REQUESTS => Err("VUT API rate limited"),
        _ => Err("VUT API request failed"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_status_distinguishes_denied_and_rate_limited_requests() {
        assert!(get_status(reqwest::StatusCode::OK).is_ok());
        assert_eq!(
            get_status(reqwest::StatusCode::FORBIDDEN),
            Err("VUT API access denied")
        );
        assert_eq!(
            get_status(reqwest::StatusCode::TOO_MANY_REQUESTS),
            Err("VUT API rate limited")
        );
        assert_eq!(
            get_status(reqwest::StatusCode::INTERNAL_SERVER_ERROR),
            Err("VUT API request failed")
        );
    }
}
