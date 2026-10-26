// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    ffi::{OsStr, OsString},
    io::Write,
    time::Duration,
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::de::DeserializeOwned;
use velnor_actions_mise::{PinnedTool, PinnedToolExec, ToolCatalog};

use crate::{OrchestratorError, internal::internal};

const API_VERSION: &str = "2026-03-10";
const JSON_LIMIT: usize = 8 * 1024 * 1024;
const API_TIMEOUT: Duration = Duration::from_secs(90);

pub(super) struct TapGitHubApi {
    catalog: ToolCatalog,
    repository: String,
    token: String,
}

impl TapGitHubApi {
    pub(super) fn new(repository: String, token: String) -> Self {
        Self {
            catalog: ToolCatalog::pinned(),
            repository,
            token,
        }
    }

    pub(super) fn repo_route(&self, suffix: &str) -> String {
        format!("repos/{}/{suffix}", self.repository)
    }

    pub(super) fn get_json<T: DeserializeOwned>(
        &self,
        route: &str,
    ) -> Result<T, OrchestratorError> {
        serde_json::from_slice(&self.request(vec![route.into()], JSON_LIMIT)?)
            .map_err(|_| internal("tap_writer_api_json"))
    }

    pub(super) fn graphql(
        &self,
        query: &str,
        fields: &[(&str, String)],
    ) -> Result<serde_json::Value, OrchestratorError> {
        let mut args = vec![
            OsString::from("graphql"),
            OsString::from("--raw-field"),
            OsString::from(format!("query={query}")),
        ];
        for (name, value) in fields {
            args.push(OsString::from("--field"));
            args.push(OsString::from(format!("{name}={value}")));
        }
        serde_json::from_slice(&self.request(args, JSON_LIMIT)?)
            .map_err(|_| internal("tap_writer_api_json"))
    }

    pub(super) fn post_fields<T: DeserializeOwned>(
        &self,
        route: &str,
        fields: &[(&str, String)],
    ) -> Result<T, OrchestratorError> {
        let mut args = vec![
            OsString::from(route),
            OsString::from("--method"),
            OsString::from("POST"),
        ];
        for (key, value) in fields {
            args.push(OsString::from("--field"));
            args.push(OsString::from(format!("{key}={value}")));
        }
        serde_json::from_slice(&self.request(args, JSON_LIMIT)?)
            .map_err(|_| internal("tap_writer_api_json"))
    }

    pub(super) fn post_blob<T: DeserializeOwned>(
        &self,
        route: &str,
        bytes: &[u8],
    ) -> Result<T, OrchestratorError> {
        let body = serde_json::json!({
            "content": STANDARD.encode(bytes),
            "encoding": "base64",
        });
        let mut request_file = tempfile::NamedTempFile::new()
            .map_err(|_| internal("tap_writer_blob_request_create"))?;
        serde_json::to_writer(&mut request_file, &body)
            .map_err(|_| internal("tap_writer_blob_request_encode"))?;
        request_file
            .flush()
            .map_err(|_| internal("tap_writer_blob_request_flush"))?;
        let args = vec![
            OsString::from(route),
            OsString::from("--method"),
            OsString::from("POST"),
            OsString::from("--input"),
            request_file.path().as_os_str().to_owned(),
        ];
        let raw = self.request(args, JSON_LIMIT)?;
        serde_json::from_slice(&raw).map_err(|_| internal("tap_writer_api_json"))
    }

    fn request(&self, mut args: Vec<OsString>, limit: usize) -> Result<Vec<u8>, OrchestratorError> {
        args = crate::github_authority_read::for_request(args)?;
        let mut gh_args = vec![
            OsString::from("api"),
            OsString::from("-H"),
            OsString::from("Accept: application/vnd.github+json"),
            OsString::from("-H"),
            OsString::from(format!("X-GitHub-Api-Version: {API_VERSION}")),
        ];
        gh_args.append(&mut args);
        let request = PinnedToolExec::new(vec![PinnedTool::Gh], OsStr::new("gh"), gh_args)
            .map_err(|_| internal("tap_writer_gh_unavailable"))?;
        let output = request
            .command_with_github_token(&self.catalog, &self.token)
            .and_then(|command| command.run_bounded(limit, API_TIMEOUT))
            .map_err(|_| internal("tap_writer_api_failed"))?;
        if !output.success {
            return Err(internal("tap_writer_api_failed"));
        }
        Ok(output.stdout)
    }
}

#[derive(Debug, serde::Deserialize)]
pub(super) struct AppResponse {
    pub id: u64,
    pub slug: String,
}

#[derive(Debug, serde::Deserialize)]
pub(super) struct UserResponse {
    pub id: u64,
    pub login: String,
}

#[derive(Debug, serde::Deserialize)]
pub(super) struct RepositoryResponse {
    pub id: u64,
    pub full_name: String,
}

#[derive(Debug, serde::Deserialize)]
pub(super) struct InstallationRepositoriesResponse {
    pub total_count: usize,
    pub repositories: Vec<RepositoryResponse>,
}

#[derive(Debug, serde::Deserialize)]
pub(super) struct RefResponse {
    pub object: RefObject,
}

#[derive(Debug, serde::Deserialize)]
pub(super) struct GitRefResponse {
    #[serde(rename = "ref")]
    pub ref_name: String,
    pub object: RefObject,
}

#[derive(Debug, serde::Deserialize)]
pub(super) struct RefObject {
    pub sha: String,
    #[serde(rename = "type")]
    pub object_type: String,
}

#[derive(Debug, serde::Deserialize)]
pub(super) struct TreeResponse {
    pub sha: String,
    pub truncated: bool,
    pub tree: Vec<TreeEntry>,
}

#[derive(Debug, serde::Deserialize)]
pub(super) struct TreeEntry {
    pub path: String,
    pub mode: String,
    #[serde(rename = "type")]
    pub object_type: String,
    pub sha: String,
}

#[derive(Debug, serde::Deserialize)]
pub(super) struct BlobResponse {
    pub sha: String,
    pub encoding: String,
    pub content: String,
}

#[derive(Debug, serde::Deserialize)]
pub(super) struct CommitObject {
    pub sha: String,
}

#[derive(Debug, serde::Deserialize)]
pub(super) struct CommitResponse {
    pub sha: String,
    pub tree: CommitObject,
    pub parents: Vec<CommitObject>,
    pub message: String,
}

#[derive(Debug, serde::Deserialize)]
pub(super) struct PullRequestResponse {
    pub number: u64,
    pub title: String,
    pub body: Option<String>,
    pub state: String,
    #[serde(default)]
    pub merged: Option<bool>,
    pub user: UserResponse,
    pub merged_at: Option<String>,
    pub merge_commit_sha: Option<String>,
    pub head: PullRequestRef,
    pub base: PullRequestRef,
}

#[derive(Debug, serde::Deserialize)]
pub(super) struct PullRequestRef {
    pub sha: String,
    pub label: String,
    #[serde(rename = "ref")]
    pub ref_name: String,
    pub repo: Option<PullRequestRepository>,
}

#[derive(Debug, serde::Deserialize)]
pub(super) struct PullRequestRepository {
    pub id: u64,
    pub full_name: String,
}

#[derive(Debug, serde::Deserialize)]
pub(super) struct BlobCreateResponse {
    pub sha: String,
}

#[derive(Debug, serde::Deserialize)]
pub(super) struct TreeCreateResponse {
    pub sha: String,
}

#[derive(Debug, serde::Deserialize)]
pub(super) struct CommitCreateResponse {
    pub sha: String,
    pub tree: CommitObject,
    pub parents: Vec<CommitObject>,
    pub message: String,
}

pub(super) fn encode_segment(value: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut output = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
            output.push(char::from(byte));
        } else {
            output.push('%');
            output.push(char::from(HEX[usize::from(byte >> 4)]));
            output.push(char::from(HEX[usize::from(byte & 0x0f)]));
        }
    }
    output
}

#[cfg(test)]
#[path = "tap_writer_github_tests.rs"]
mod tests;
