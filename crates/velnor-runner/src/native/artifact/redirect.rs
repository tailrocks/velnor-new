// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use anyhow::{Context, Result};
use reqwest::header::LOCATION;
use url::Url;

use crate::protocol::http::validate_authenticated_url;

pub(super) const MAX_REDIRECTS: usize = 50;

pub(super) fn follow(location: &str) -> Result<Url> {
    validate_authenticated_url(location).context("validate the artifact redirect URL")
}

pub(super) fn target(response: &reqwest::Response) -> Result<Option<Url>> {
    if !matches!(response.status().as_u16(), 301 | 302 | 303 | 307 | 308) {
        return Ok(None);
    }
    response
        .headers()
        .get(LOCATION)
        .map(|value| {
            let text: String = value
                .as_bytes()
                .iter()
                .map(|&byte| char::from(byte))
                .collect();
            follow(&text)
        })
        .transpose()
}

#[cfg(test)]
mod tests {
    use super::follow;

    #[test]
    fn only_an_absolute_https_location_is_followed() {
        assert!(follow("https://blob.example/next?sig=1").is_ok());
        for refused in [
            "http://blob.example/next",
            "/next",
            "next",
            "",
            "https://user:pass@blob.example/next",
        ] {
            assert!(follow(refused).is_err(), "{refused}");
        }
    }
}
