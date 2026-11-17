// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::CiError;
use crate::github::{GitHub, Repo};

pub(super) fn verify(key: &str) -> Result<String, CiError> {
    if key.is_empty() {
        return Err(CiError::new("cache key is empty"));
    }
    let repo = std::env::var("GITHUB_REPOSITORY").map_err(|e| CiError::new(e.to_string()))?;
    let repo = Repo::parse(&repo)?;
    let reference = std::env::var("GITHUB_REF").map_err(|e| CiError::new(e.to_string()))?;
    let github = super::http::HttpGitHub::new(
        super::api_url(),
        super::token()?,
        super::http::DEFAULT_TIMEOUT,
    );
    let listing = github.get(&format!(
        "/repos/{repo}/actions/caches?key={}&ref={}&per_page=100",
        encode(key),
        encode(&reference)
    ))?;
    if crate::cache::parse_entries(&listing)?
        .iter()
        .any(|entry| entry.key == key)
    {
        Ok(format!("cache upload verified: {key}\n"))
    } else {
        Err(CiError::new(format!("cache {key} was not saved")))
    }
}

fn encode(value: &str) -> String {
    value
        .bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
                char::from(byte).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect()
}

pub(super) fn check(success: &str) -> Result<String, CiError> {
    if success == "true" {
        Ok("cache saves verified\n".to_owned())
    } else {
        Err(CiError::new(
            "a cache save failed; inspect the lane upload verification",
        ))
    }
}
