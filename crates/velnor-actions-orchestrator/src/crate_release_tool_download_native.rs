// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    Instant, OrchestratorError, ToolDownloadResponse, ToolDownloadTransport, internal,
    validate_storage_url,
};

#[derive(Debug, Default)]
pub struct NativeToolDownloadTransport;

impl ToolDownloadTransport for NativeToolDownloadTransport {
    fn github(
        &self,
        repository: &str,
        id: u64,
        token: &str,
        deadline: Instant,
    ) -> Result<ToolDownloadResponse, OrchestratorError> {
        let Some((owner, name)) = repository.split_once('/') else {
            return Err(internal("crate_release_tool_download_invalid"));
        };
        if !velnor_actions_contract::config::is_valid_github_owner(owner)
            || !velnor_actions_contract::config::is_valid_github_repo_name(name)
            || id == 0
            || token.is_empty()
            || token.bytes().any(|byte| byte.is_ascii_control())
        {
            return Err(internal("crate_release_tool_download_invalid"));
        }
        let url = format!("https://api.github.com/repos/{repository}/actions/artifacts/{id}/zip");
        let request = crate::api::github_evidence_http::agent()?
            .get(url)
            .header("Authorization", format!("Bearer {token}"))
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .header("Accept-Encoding", "identity")
            .header("Cache-Control", "no-cache, no-store")
            .header("User-Agent", "velnor-actions-tool-artifact");
        native_download_request(request, deadline)
    }
    fn storage(
        &self,
        url: &str,
        deadline: Instant,
    ) -> Result<ToolDownloadResponse, OrchestratorError> {
        let safe_url = validate_storage_url(url)?;
        let request = crate::api::github_evidence_http::agent()?
            .get(safe_url)
            .header("Accept-Encoding", "identity")
            .header("User-Agent", "velnor-actions-tool-artifact");
        native_download_request(request, deadline)
    }
}

pub(super) fn native_download_request(
    request: ureq::RequestBuilder<ureq::typestate::WithoutBody>,
    deadline: Instant,
) -> Result<ToolDownloadResponse, OrchestratorError> {
    let invalid = || internal("crate_release_tool_download_transport");
    let remaining = deadline
        .checked_duration_since(Instant::now())
        .filter(|duration| !duration.is_zero())
        .ok_or_else(invalid)?;
    let response = request
        .config()
        .timeout_global(Some(remaining))
        .max_redirects(0)
        .http_status_as_error(false)
        .build()
        .call()
        .map_err(|_| invalid())?;
    let headers = response.headers();
    let single = |name: &str| -> Result<Option<String>, OrchestratorError> {
        let mut values = headers.get_all(name).iter();
        let first = values
            .next()
            .map(|value| value.to_str().map(str::to_owned).map_err(|_| invalid()))
            .transpose()?;
        if values.next().is_some() {
            return Err(invalid());
        }
        Ok(first)
    };
    let location = single("location")?;
    let content_encoding = single("content-encoding")?;
    let content_length = single("content-length")?
        .map(|value| value.parse().map_err(|_| invalid()))
        .transpose()?;
    let status = response.status().as_u16();
    Ok(ToolDownloadResponse {
        status,
        location,
        content_length,
        content_encoding,
        body: Box::new(response.into_body().into_reader()),
    })
}
