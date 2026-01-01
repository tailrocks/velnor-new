//! Repository-scoped pinned `gh api` requests.

use std::ffi::{OsStr, OsString};
use std::time::Duration;

use serde::de::DeserializeOwned;
use velnor_actions_mise::{PinnedTool, PinnedToolExec, ToolCatalog};

use crate::OrchestratorError;
use crate::internal::internal;
use crate::origin::validate_repository_slug;

const MAX_API_JSON_BYTES: usize = 1_048_576;
const GH_API_TIMEOUT: Duration = Duration::from_secs(60);
const UPPER_HEX: &[u8; 16] = b"0123456789ABCDEF";

pub(in crate::qualification_resolver) struct GitHub<'a> {
    catalog: &'a ToolCatalog,
    repository: String,
    default_branch: String,
}

impl<'a> GitHub<'a> {
    pub(in crate::qualification_resolver) fn for_repository(
        catalog: &'a ToolCatalog,
        repository: &str,
    ) -> Result<Self, OrchestratorError> {
        let repository = validate_repository_slug(repository)
            .ok_or_else(|| internal("qualification_repository_invalid"))?;
        let route = format!("repos/{repository}");
        let repo: RepositoryResponse = Self::json(catalog, &route)?;
        if repo.full_name.as_deref() != Some(repository.as_str()) {
            return Err(internal("qualification_repository_mismatch"));
        }
        let default_branch = required(repo.default_branch, "qualification_default_branch")?;
        validate_branch_name(&default_branch)?;
        Ok(Self {
            catalog,
            repository,
            default_branch,
        })
    }

    pub(in crate::qualification_resolver) fn repository(&self) -> &str {
        &self.repository
    }

    pub(in crate::qualification_resolver) fn catalog(&self) -> &ToolCatalog {
        self.catalog
    }

    pub(in crate::qualification_resolver) fn default_branch(&self) -> &str {
        &self.default_branch
    }

    pub(in crate::qualification_resolver) fn validate_current_branch(
        &self,
        expected: &str,
    ) -> Result<(), OrchestratorError> {
        if expected != self.default_branch {
            return Err(internal("qualification_default_branch_mismatch"));
        }
        let branch = percent_encode_path_segment(expected);
        let route = format!("repos/{}/branches/{branch}", self.repository);
        let response: BranchResponse = Self::json(self.catalog, &route)?;
        if response.protected != Some(true) {
            return Err(internal("qualification_default_branch_unprotected"));
        }
        Ok(())
    }

    pub(in crate::qualification_resolver) fn json<T: DeserializeOwned>(
        catalog: &ToolCatalog,
        route: &str,
    ) -> Result<T, OrchestratorError> {
        let bytes = Self::api_bytes(catalog, route)?;
        serde_json::from_slice(&bytes).map_err(|_| internal("qualification_api_json"))
    }

    pub(in crate::qualification_resolver) fn api_bytes(
        catalog: &ToolCatalog,
        route: &str,
    ) -> Result<Vec<u8>, OrchestratorError> {
        if route.is_empty() || route.starts_with('/') || route.contains("..") {
            return Err(internal("qualification_api_route_invalid"));
        }
        let args = vec![OsString::from("api"), OsString::from(route)];
        let exec = PinnedToolExec::new(vec![PinnedTool::Gh], OsStr::new("gh"), args)
            .map_err(|_| internal("qualification_gh_unavailable"))?;
        let output = exec
            .command(catalog)
            .and_then(|command| command.run_bounded(MAX_API_JSON_BYTES, GH_API_TIMEOUT))
            .map_err(|_| internal("qualification_api_unavailable"))?;
        if !output.success {
            return Err(internal("qualification_api_unavailable"));
        }
        Ok(output.stdout)
    }
}

#[derive(serde::Deserialize)]
struct RepositoryResponse {
    full_name: Option<String>,
    default_branch: Option<String>,
}

#[derive(serde::Deserialize)]
struct BranchResponse {
    protected: Option<bool>,
}

fn required(value: Option<String>, error: &'static str) -> Result<String, OrchestratorError> {
    value
        .filter(|value| !value.is_empty())
        .ok_or_else(|| internal(error))
}

fn validate_branch_name(value: &str) -> Result<(), OrchestratorError> {
    if value.trim().is_empty()
        || value.starts_with('-')
        || value.contains("..")
        || value.chars().any(char::is_control)
    {
        return Err(internal("qualification_default_branch_invalid"));
    }
    Ok(())
}

fn percent_encode_path_segment(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
            encoded.push(char::from(byte));
        } else {
            encoded.push('%');
            encoded.push(char::from(UPPER_HEX[usize::from(byte >> 4)]));
            encoded.push(char::from(UPPER_HEX[usize::from(byte & 0x0f)]));
        }
    }
    encoded
}
