// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Duration;

use crate::error::CiError;
use crate::github::GitHub;

const API: &str = "https://api.github.com";
const BODY_LIMIT: u64 = 512 * 1024 * 1024;

#[derive(Debug)]
pub(super) struct HttpGitHub {
    agent: ureq::Agent,
    token: String,
}

impl HttpGitHub {
    pub(super) fn new(token: String) -> Self {
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(300)))
            .build()
            .into();
        Self { agent, token }
    }
}

impl GitHub for HttpGitHub {
    fn get(&self, path: &str) -> Result<String, CiError> {
        let url = format!("{API}{path}");
        let mut response = self
            .agent
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.token))
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .header("User-Agent", "velnor-ci")
            .call()
            .map_err(|e| CiError::new(format!("GET {path}: {e}")))?;
        let bytes = super::read_limited(response.body_mut().as_reader(), BODY_LIMIT)
            .map_err(|e| CiError::new(format!("GET {path}: {e}")))?;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }
}
