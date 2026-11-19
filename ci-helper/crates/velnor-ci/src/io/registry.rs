// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::Deserialize;

use super::http::DEFAULT_TIMEOUT;
use crate::error::CiError;
use crate::release_published::Registry;

const INDEX: &str = "https://index.crates.io";
const BODY_LIMIT: u64 = 64 * 1024 * 1024;

pub(super) struct CratesIo {
    agent: ureq::Agent,
}

impl CratesIo {
    pub(super) fn new() -> Self {
        Self {
            agent: ureq::Agent::config_builder()
                .timeout_global(Some(DEFAULT_TIMEOUT))
                .build()
                .into(),
        }
    }
}

#[derive(Deserialize)]
struct Entry {
    vers: String,
}

fn index_path(name: &str) -> String {
    let lower = name.to_ascii_lowercase();
    match lower.len() {
        1 | 2 => format!("{}/{lower}", lower.len()),
        3 => format!("3/{}/{lower}", &lower[..1]),
        _ => format!("{}/{}/{lower}", &lower[..2], &lower[2..4]),
    }
}

impl Registry for CratesIo {
    fn is_published(&self, package: &str, version: &str) -> Result<bool, CiError> {
        let url = format!("{INDEX}/{}", index_path(package));
        let mut response = match self
            .agent
            .get(&url)
            .header("User-Agent", "velnor-ci")
            .call()
        {
            Ok(response) => response,
            Err(ureq::Error::StatusCode(404)) => return Ok(false),
            Err(e) => return Err(CiError::new(format!("GET {url}: {e}"))),
        };
        let bytes = super::read_limited(response.body_mut().as_reader(), BODY_LIMIT)
            .map_err(|e| CiError::new(format!("GET {url}: {e}")))?;
        for line in String::from_utf8_lossy(&bytes).lines() {
            let entry: Entry = serde_json::from_str(line)
                .map_err(|e| CiError::new(format!("{url}: not a crates.io index entry: {e}")))?;
            if entry.vers == version {
                return Ok(true);
            }
        }
        Ok(false)
    }
}
