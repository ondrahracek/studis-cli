//! HTTP client settings shared by the token and studies requests.

use std::time::Duration;

pub(crate) fn client() -> Result<reqwest::blocking::Client, &'static str> {
    reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(20))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| "unable to initialize VUT HTTP client")
}
