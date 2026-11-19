// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use ureq::http::Response;

use super::http::DEFAULT_TIMEOUT;
use crate::error::CiError;
use crate::github::Repo;
use crate::reports::{Listed, ListedPage};

const PAGE_SIZE: usize = 100;
const PAGE_LIMIT: usize = 50;
const LISTING_LIMIT: u64 = 16 << 20;
const ARCHIVE_LIMIT: u64 = 256 << 20;

#[derive(Debug, Clone)]
pub(super) struct ArtifactApi {
    agent: ureq::Agent,
    api: String,
    token: String,
}

impl ArtifactApi {
    pub(super) fn new(api: String, token: String) -> Self {
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(DEFAULT_TIMEOUT))
            .build()
            .into();
        Self { agent, api, token }
    }

    fn fetch(&self, path: &str, limit: u64) -> Result<Vec<u8>, CiError> {
        let url = format!("{}{path}", self.api);
        let response: Result<Response<ureq::Body>, ureq::Error> = self
            .agent
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.token))
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .header("User-Agent", "velnor-ci")
            .call();
        let mut response = response.map_err(|e| {
            let message = format!("GET {path}: {e}");
            if matches!(e, ureq::Error::StatusCode(404 | 410)) {
                CiError::missing(message)
            } else {
                CiError::new(message)
            }
        })?;
        super::read_limited(response.body_mut().as_reader(), limit)
            .map_err(|e| e.context(&format!("GET {path}")))
    }

    pub(super) fn list(&self, repo: &Repo, run_id: u64) -> Result<Vec<Listed>, CiError> {
        let mut listed = Vec::new();
        for page in 1..=PAGE_LIMIT {
            let path = format!(
                "/repos/{repo}/actions/runs/{run_id}/artifacts?per_page={PAGE_SIZE}&page={page}"
            );
            let bytes = self.fetch(&path, LISTING_LIMIT)?;
            let parsed: ListedPage = serde_json::from_slice(&bytes)
                .map_err(|e| CiError::new(format!("GET {path}: not an artifact listing: {e}")))?;
            let empty = parsed.artifacts.is_empty();
            listed.extend(parsed.artifacts);
            if empty || listed.len() >= parsed.total_count {
                return Ok(listed);
            }
        }
        Err(CiError::new(format!(
            "run {run_id} lists more than {} artifacts",
            PAGE_LIMIT * PAGE_SIZE
        )))
    }

    pub(super) fn download(&self, repo: &Repo, id: u64) -> Result<Vec<u8>, CiError> {
        self.fetch(
            &format!("/repos/{repo}/actions/artifacts/{id}/zip"),
            ARCHIVE_LIMIT,
        )
    }
}
