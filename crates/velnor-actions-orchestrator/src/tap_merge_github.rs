//! Fixed, bounded GitHub API transport for exact-head tap merge admission.
//!
//! REST request shapes follow GitHub's current public REST API. Review-thread
//! pagination uses the documented GraphQL `PullRequest.reviewThreads` edge.

#[cfg(test)]
use std::cell::RefCell;
use std::{
    ffi::{OsStr, OsString},
    time::{Duration, Instant},
};

use serde::de::DeserializeOwned;
use velnor_actions_mise::{PinnedTool, PinnedToolExec, ToolCatalog};

use crate::{OrchestratorError, internal::internal};

#[cfg(test)]
type TestTransport = dyn FnMut(&[OsString]) -> Result<ApiResponse, OrchestratorError>;

const API_VERSION: &str = "2026-03-10";
const RESPONSE_LIMIT: usize = 16 * 1024 * 1024;
const API_TIMEOUT: Duration = Duration::from_secs(90);

pub(super) struct TapMergeGitHub {
    catalog: ToolCatalog,
    token: String,
    deadline: Instant,
    #[cfg(test)]
    test_transport: Option<RefCell<Box<TestTransport>>>,
}

pub(super) struct ApiResponse {
    pub(super) status: u16,
    pub(super) body: Vec<u8>,
}

impl TapMergeGitHub {
    pub(super) fn with_max_wait(token: String, max_wait: Duration) -> Self {
        Self {
            catalog: ToolCatalog::pinned(),
            token,
            deadline: Instant::now() + max_wait,
            #[cfg(test)]
            test_transport: None,
        }
    }

    #[cfg(test)]
    pub(super) fn with_test_transport(
        transport: impl FnMut(&[OsString]) -> Result<ApiResponse, OrchestratorError> + 'static,
    ) -> Self {
        Self {
            catalog: ToolCatalog::pinned(),
            token: "test-only-token".to_owned(),
            deadline: Instant::now() + Duration::from_secs(60),
            test_transport: Some(RefCell::new(Box::new(transport))),
        }
    }

    pub(super) fn deadline(&self) -> Instant {
        self.deadline
    }

    pub(super) fn get<T: DeserializeOwned>(&self, route: &str) -> Result<T, OrchestratorError> {
        decode_ok(&self.request(vec![route.into()])?)
    }

    pub(super) fn get_optional<T: DeserializeOwned>(
        &self,
        route: &str,
    ) -> Result<Option<T>, OrchestratorError> {
        let response = self.request(vec![route.into()])?;
        if response.status == 404 {
            return Ok(None);
        }
        decode_ok(&response).map(Some)
    }

    pub(super) fn graphql<T: DeserializeOwned>(
        &self,
        query: &str,
        fields: &[(&str, String)],
    ) -> Result<T, OrchestratorError> {
        let mut args = vec![
            OsString::from("graphql"),
            OsString::from("--raw-field"),
            OsString::from(format!("query={query}")),
        ];
        for (name, value) in fields {
            args.push(OsString::from("--field"));
            args.push(OsString::from(format!("{name}={value}")));
        }
        decode_ok(&self.request(args)?)
    }

    pub(super) fn put_fields<T: DeserializeOwned>(
        &self,
        route: &str,
        fields: &[(&str, String)],
    ) -> Result<T, OrchestratorError> {
        let mut args = vec![
            OsString::from(route),
            OsString::from("--method"),
            OsString::from("PUT"),
        ];
        for (name, value) in fields {
            args.push(OsString::from("--raw-field"));
            args.push(OsString::from(format!("{name}={value}")));
        }
        decode_ok(&self.request(args)?)
    }

    pub(super) fn merge(
        &self,
        repository: &str,
        pull_request: u64,
        head_sha: &str,
    ) -> Result<serde_json::Value, OrchestratorError> {
        self.put_fields(
            &format!("repos/{repository}/pulls/{pull_request}/merge"),
            &[
                ("sha", head_sha.to_owned()),
                ("merge_method", "squash".to_owned()),
            ],
        )
    }

    fn request(&self, mut args: Vec<OsString>) -> Result<ApiResponse, OrchestratorError> {
        #[cfg(test)]
        if let Some(transport) = &self.test_transport {
            return (transport.borrow_mut())(&args);
        }
        let mut gh_args = vec![
            OsString::from("api"),
            OsString::from("--include"),
            OsString::from("-H"),
            OsString::from("Accept: application/vnd.github+json"),
            OsString::from("-H"),
            OsString::from(format!("X-GitHub-Api-Version: {API_VERSION}")),
        ];
        gh_args.append(&mut args);
        let command = PinnedToolExec::new(vec![PinnedTool::Gh], OsStr::new("gh"), gh_args)
            .map_err(|_| internal("tap_merge_gh_unavailable"))?
            .command_with_github_token(&self.catalog, &self.token)
            .map_err(|_| internal("tap_merge_gh_unavailable"))?;
        let timeout = API_TIMEOUT.min(self.deadline.saturating_duration_since(Instant::now()));
        if timeout.is_zero() {
            return Err(internal("tap_merge_deadline_elapsed"));
        }
        let output = command
            .run_bounded(RESPONSE_LIMIT, timeout)
            .map_err(|_| internal("tap_merge_api_failed"))?;
        parse_included_response(&output.stdout)
    }
}

fn decode_ok<T: DeserializeOwned>(response: &ApiResponse) -> Result<T, OrchestratorError> {
    if response.status != 200 {
        return Err(internal("tap_merge_api_status"));
    }
    serde_json::from_slice(&response.body).map_err(|_| internal("tap_merge_api_json"))
}

fn parse_included_response(bytes: &[u8]) -> Result<ApiResponse, OrchestratorError> {
    let text = std::str::from_utf8(bytes).map_err(|_| internal("tap_merge_api_headers"))?;
    let (headers, body) = text
        .rsplit_once("\r\n\r\n")
        .or_else(|| text.rsplit_once("\n\n"))
        .ok_or_else(|| internal("tap_merge_api_headers"))?;
    let status_line = headers
        .lines()
        .rev()
        .find(|line| line.starts_with("HTTP/"))
        .ok_or_else(|| internal("tap_merge_api_status"))?;
    let status = status_line
        .split_ascii_whitespace()
        .nth(1)
        .and_then(|value| value.parse::<u16>().ok())
        .filter(|value| (200..600).contains(value))
        .ok_or_else(|| internal("tap_merge_api_status"))?;
    Ok(ApiResponse {
        status,
        body: body.as_bytes().to_vec(),
    })
}

#[cfg(test)]
#[path = "tap_merge_github_tests.rs"]
mod tests;
