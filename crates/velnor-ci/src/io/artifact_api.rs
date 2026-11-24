// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

use ureq::http::Response;

use super::http::DEFAULT_TIMEOUT;
use crate::error::CiError;
use crate::github::Repo;
use crate::reports::{Listed, ListedPage};

const PAGE_SIZE: usize = 100;
const PAGE_LIMIT: usize = 50;
const LISTING_LIMIT: u64 = 16 << 20;
const ARCHIVE_LIMIT: u64 = 256 << 20;
const ATTEMPTS: u32 = 3;

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

    fn get(&self, path: &str) -> Result<Response<ureq::Body>, CiError> {
        let url = format!("{}{path}", self.api);
        let response: Result<Response<ureq::Body>, ureq::Error> = self
            .agent
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.token))
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .header("User-Agent", "velnor-ci")
            .call();
        response.map_err(|e| {
            let message = format!("GET {path}: {e}");
            // A 404 can be a listed artifact that is not readable yet, so it is retried; a 410
            // says the artifact has expired for good.
            if matches!(e, ureq::Error::StatusCode(410)) {
                CiError::missing(message)
            } else {
                CiError::new(message)
            }
        })
    }

    fn fetch(&self, path: &str, limit: u64) -> Result<Vec<u8>, CiError> {
        let mut response = self.get(path)?;
        super::read_limited(response.body_mut().as_reader(), limit)
            .map_err(|e| e.context(&format!("GET {path}")))
    }

    fn fetch_to_file(&self, path: &str, limit: u64, scratch: &Path) -> Result<File, CiError> {
        let mut response = self.get(path)?;
        let context = |e: &dyn std::fmt::Display| CiError::new(format!("GET {path}: {e}"));
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(scratch)
            .map_err(|e| context(&format!("cannot create {}: {e}", scratch.display())))?;
        fs::remove_file(scratch)
            .map_err(|e| context(&format!("cannot unlink {}: {e}", scratch.display())))?;
        let copied = io::copy(
            &mut response.body_mut().as_reader().take(limit + 1),
            &mut file,
        )
        .map_err(|e| context(&e))?;
        if copied > limit {
            return Err(context(&format!("response is larger than {limit} bytes")));
        }
        file.seek(SeekFrom::Start(0)).map_err(|e| context(&e))?;
        Ok(file)
    }

    fn retrying<T>(mut attempt_once: impl FnMut() -> Result<T, CiError>) -> Result<T, CiError> {
        let mut attempt = 1;
        loop {
            match attempt_once() {
                Ok(value) => return Ok(value),
                Err(error) if error.is_missing() || attempt == ATTEMPTS => {
                    return Err(error.context(&format!("failed after {attempt} attempts")));
                }
                Err(_) => attempt += 1,
            }
        }
    }

    fn fetch_retrying(&self, path: &str, limit: u64) -> Result<Vec<u8>, CiError> {
        Self::retrying(|| self.fetch(path, limit))
    }

    pub(super) fn list(&self, repo: &Repo, run_id: u64) -> Result<Vec<Listed>, CiError> {
        let mut listed = Vec::new();
        for page in 1..=PAGE_LIMIT {
            let path = format!(
                "/repos/{repo}/actions/runs/{run_id}/artifacts?per_page={PAGE_SIZE}&page={page}"
            );
            let bytes = self.fetch_retrying(&path, LISTING_LIMIT)?;
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

    /// Streams the archive into `scratch`, an unlinked file, so no archive is held in memory.
    pub(super) fn download(&self, repo: &Repo, id: u64, scratch: &Path) -> Result<File, CiError> {
        let path = format!("/repos/{repo}/actions/artifacts/{id}/zip");
        Self::retrying(|| self.fetch_to_file(&path, ARCHIVE_LIMIT, scratch))
    }
}
