//! Fixed GitHub REST operations for the installation-token tap writer.

#[cfg(test)]
use std::cell::RefCell;
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
    #[cfg(test)]
    scripted: Option<RefCell<std::collections::VecDeque<ScriptedResponse>>>,
}

impl TapGitHubApi {
    pub(super) fn new(repository: String, token: String) -> Self {
        Self {
            catalog: ToolCatalog::pinned(),
            repository,
            token,
            #[cfg(test)]
            scripted: None,
        }
    }

    #[cfg(test)]
    pub(super) fn scripted(
        repository: String,
        responses: impl IntoIterator<Item = ScriptedResponse>,
    ) -> Self {
        Self {
            catalog: ToolCatalog::pinned(),
            repository,
            token: "test-only-token".to_owned(),
            scripted: Some(RefCell::new(responses.into_iter().collect())),
        }
    }

    #[cfg(test)]
    pub(super) fn scripted_is_empty(&self) -> bool {
        self.scripted
            .as_ref()
            .is_some_and(|responses| responses.borrow().is_empty())
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
        #[cfg(test)]
        if let Some(scripted) = &self.scripted {
            let route = args
                .first()
                .and_then(|value| value.to_str())
                .ok_or_else(|| internal("tap_writer_test_route_missing"))?;
            let method = args
                .windows(2)
                .find(|window| window[0] == "--method")
                .and_then(|window| window[1].to_str())
                .unwrap_or("GET");
            let response = scripted
                .borrow_mut()
                .pop_front()
                .ok_or_else(|| internal("tap_writer_test_unexpected_request"))?;
            if response.route != route || response.method != method || response.body.len() > limit {
                return Err(internal("tap_writer_test_request_mismatch"));
            }
            return Ok(response.body);
        }
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

#[cfg(test)]
pub(super) struct ScriptedResponse {
    pub(super) method: &'static str,
    pub(super) route: String,
    pub(super) body: Vec<u8>,
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
pub(super) struct CommitResponse {
    pub sha: String,
    pub tree: RefObject,
    pub parents: Vec<RefObject>,
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
    pub tree: RefObject,
    pub parents: Vec<RefObject>,
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
mod tests {
    use super::{PullRequestResponse, ScriptedResponse, TapGitHubApi};

    fn pull_request_json() -> serde_json::Value {
        serde_json::json!({
            "number": 7,
            "title": "Update preview formula",
            "body": "candidate proof",
            "state": "closed",
            "merged_at": "2026-10-18T10:00:00Z",
            "merge_commit_sha": "a".repeat(40),
            "user": {"id": 11, "login": "writer[bot]"},
            "head": {
                "sha": "b".repeat(40),
                "label": "owner:velnor/preview/branch",
                "ref": "velnor/preview/branch",
                "repo": {"full_name": "owner/tap"}
            },
            "base": {
                "sha": "c".repeat(40),
                "label": "owner:main",
                "ref": "main",
                "repo": {"full_name": "owner/tap"}
            }
        })
    }

    #[test]
    fn pull_list_may_omit_merged_but_detail_must_supply_it() {
        let summary: PullRequestResponse =
            serde_json::from_value(pull_request_json()).expect("list summary");
        assert_eq!(summary.merged, None);

        let mut detail = pull_request_json();
        detail["merged"] = serde_json::json!(true);
        let detail: PullRequestResponse = serde_json::from_value(detail).expect("detail");
        assert_eq!(detail.merged, Some(true));
    }

    #[test]
    fn scripted_api_rejects_unexpected_mutation_after_read_only_snapshot() {
        let route = "repos/owner/tap/pulls?state=all&per_page=100&page=1";
        let api = TapGitHubApi::scripted(
            "owner/tap".to_owned(),
            [ScriptedResponse {
                method: "GET",
                route: route.to_owned(),
                body: b"[]".to_vec(),
            }],
        );
        let pulls: Vec<PullRequestResponse> = api.get_json(route).expect("pull list");
        assert!(pulls.is_empty());
        assert!(api.scripted_is_empty());
        assert!(
            api.post_fields::<serde_json::Value>("repos/owner/tap/pulls", &[])
                .is_err()
        );
    }
}
