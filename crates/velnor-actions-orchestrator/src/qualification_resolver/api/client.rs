//! Repository-scoped pinned `gh api` requests.

use std::ffi::{OsStr, OsString};
use std::time::Duration;

use serde::de::DeserializeOwned;
use velnor_actions_contract::{QualificationDispatch, QualificationDispatchMode};
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
    branch: String,
    ref_protected: bool,
    workflow_path: String,
    allow_unprotected_default: bool,
}

impl<'a> GitHub<'a> {
    pub(in crate::qualification_resolver) fn for_repository(
        catalog: &'a ToolCatalog,
        context: &QualificationDispatch,
        verification_branch: Option<&str>,
        expected_source_sha: &str,
    ) -> Result<Self, OrchestratorError> {
        let repository = validate_repository_slug(&context.repository)
            .ok_or_else(|| internal("qualification_repository_invalid"))?;
        let route = format!("repos/{repository}");
        let repo: RepositoryResponse = Self::json(catalog, &route)?;
        if repo.full_name.as_deref() != Some(repository.as_str()) {
            return Err(internal("qualification_repository_mismatch"));
        }
        let default_branch = required(repo.default_branch, "qualification_default_branch")?;
        validate_branch_name(&default_branch)?;
        if default_branch != context.default_branch {
            return Err(internal("qualification_default_branch_mismatch"));
        }
        let branch = context
            .git_ref
            .strip_prefix("refs/heads/")
            .filter(|value| !value.is_empty())
            .ok_or_else(|| internal("qualification_branch_invalid"))?
            .to_owned();
        validate_branch_name(&branch)?;
        let workflow_path = context
            .workflow_ref
            .strip_prefix(&format!("{repository}/"))
            .and_then(|value| value.split_once('@').map(|(path, _)| path))
            .filter(|path| {
                *path == velnor_actions_workflow_renderer::render::WORKFLOW_PATH
                    || *path == velnor_actions_contract::workflow::QUALIFICATION_WORKFLOW_PATH
            })
            .ok_or_else(|| internal("qualification_workflow_invalid"))?
            .to_owned();
        let allow_unprotected_default = allows_unprotected_default(
            context,
            &repository,
            &default_branch,
            verification_branch,
            expected_source_sha,
        )?;
        Ok(Self {
            catalog,
            repository,
            default_branch,
            branch,
            ref_protected: context.ref_protected,
            workflow_path,
            allow_unprotected_default,
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

    pub(in crate::qualification_resolver) fn branch(&self) -> &str {
        &self.branch
    }

    pub(in crate::qualification_resolver) fn workflow_path(&self) -> &str {
        &self.workflow_path
    }

    pub(in crate::qualification_resolver) fn ref_protected(&self) -> bool {
        self.ref_protected
    }

    pub(in crate::qualification_resolver) fn validate_current_branch(
        &self,
    ) -> Result<(), OrchestratorError> {
        let branch = percent_encode_path_segment(&self.branch);
        let route = format!("repos/{}/branches/{branch}", self.repository);
        let response: BranchResponse = Self::json(self.catalog, &route)?;
        if !branch_protection_matches(
            response.protected,
            self.ref_protected,
            self.branch == self.default_branch,
            self.allow_unprotected_default,
        ) {
            return Err(internal("qualification_branch_protection_mismatch"));
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

fn allows_unprotected_default(
    context: &QualificationDispatch,
    repository: &str,
    default_branch: &str,
    verification_branch: Option<&str>,
    expected_source_sha: &str,
) -> Result<bool, OrchestratorError> {
    context
        .validate_for_config(
            default_branch,
            verification_branch,
            repository,
            expected_source_sha,
        )
        .map_err(crate::internal::internal_contract)?;
    let workflow_path = context
        .workflow_ref
        .strip_prefix(&format!("{repository}/"))
        .and_then(|value| value.split_once('@').map(|(path, _)| path));
    Ok(context.mode == QualificationDispatchMode::VerificationOnly
        && context.git_ref == format!("refs/heads/{default_branch}")
        && workflow_path == Some(velnor_actions_contract::workflow::QUALIFICATION_WORKFLOW_PATH))
}

fn branch_protection_matches(
    api_protected: Option<bool>,
    runner_protected: bool,
    is_default_branch: bool,
    allow_unprotected_default: bool,
) -> bool {
    api_protected == Some(runner_protected)
        && (!is_default_branch || runner_protected || allow_unprotected_default)
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

#[cfg(test)]
#[path = "client_tests.rs"]
mod tests;
