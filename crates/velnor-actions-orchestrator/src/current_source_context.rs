//! Runner-owned authority for evidence taken from the current checkout.

use std::path::{Path, PathBuf};

use velnor_actions_contract::{
    Plan, VerificationScope, WorkflowEvent, digest_b3, parse_strict_json,
};

use crate::OrchestratorError;
use crate::cover_baseline::provenance_check::repository_slug_from_origin;
use crate::internal::internal;
use crate::internal_request::resolve_run_key;
use crate::request_event::{request_refs_for_scope, request_scope, workflow_event_for};
use crate::safe_read::{MAX_REPO_FILE_BYTES, read_event_file};
use crate::select::verify_checkout;
use crate::validators::validate_diff_rev;

/// An independently captured runner event bound to its tested checkout.
///
/// Private fields prevent a plan artifact from constructing authority.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct CurrentSourceContext {
    root: PathBuf,
    event_path: PathBuf,
    payload_digest: String,
    event: WorkflowEvent,
    scope: VerificationScope,
    base: Option<String>,
    event_head: String,
    candidate: String,
    github_sha: String,
    run_key: String,
    repository: String,
}

impl CurrentSourceContext {
    /// Capture runner authority and require the plan to describe it exactly.
    pub(crate) fn capture(root: &Path, plan: &Plan) -> Result<Self, OrchestratorError> {
        let event_name = required_env("GITHUB_EVENT_NAME", "missing_actual_event")?;
        let event_path = std::env::var_os("GITHUB_EVENT_PATH")
            .filter(|path| !path.is_empty())
            .map(PathBuf::from)
            .ok_or_else(|| internal("missing_actual_payload"))?;
        let text = read_event_file(&event_path, MAX_REPO_FILE_BYTES)?;
        let payload = parse_strict_json(&text).map_err(|_| internal("malformed_actual_payload"))?;
        let event = actual_event_for(&event_name, &payload)?;
        let scope = request_scope::scope_for(event, &payload)?;
        let github_sha = std::env::var("GITHUB_SHA").map_err(|_| internal("missing_actual_sha"))?;
        validate_actual_sha(Some(&github_sha))?;
        let (base, event_head) = request_refs_for_scope(event, &payload, Some(&github_sha), scope)?;
        let root = root
            .canonicalize()
            .map_err(|err| OrchestratorError::io(root.display().to_string(), err.to_string()))?;
        let candidate = verify_checkout(&root, event, base.as_deref(), &event_head)?;
        verify_candidate_sha(&github_sha, &candidate)?;
        let repository = current_repository(&root)?;
        verify_payload_repository(&payload, &repository)?;
        let context = Self {
            repository,
            run_key: resolve_run_key(None)?,
            root,
            event_path,
            payload_digest: digest_b3(text.as_bytes()),
            event,
            scope,
            base,
            event_head,
            candidate,
            github_sha,
        };
        context.verify_plan(plan)?;
        Ok(context)
    }

    /// Re-read runner channels and checkout before consuming source evidence.
    pub(crate) fn verify_current(&self, root: &Path, plan: &Plan) -> Result<(), OrchestratorError> {
        if Self::capture(root, plan)? != *self {
            return Err(internal("current_source_context_changed"));
        }
        Ok(())
    }

    /// Actual runner run key; never obtained from a serialized plan.
    pub(crate) fn run_key(&self) -> &str {
        &self.run_key
    }

    pub(crate) fn repository(&self) -> &str {
        &self.repository
    }

    pub(crate) fn candidate(&self) -> &str {
        &self.candidate
    }

    /// Bind the claimed plan to the actual candidate, event, and run.
    fn verify_plan(&self, plan: &Plan) -> Result<(), OrchestratorError> {
        for (matches, problem) in [
            (plan.event == self.event, "current_source_event_mismatch"),
            (plan.scope == self.scope, "current_source_scope_mismatch"),
            (plan.base == self.base, "current_source_base_mismatch"),
            (plan.head == self.candidate, "current_source_head_mismatch"),
            (plan.run_key == self.run_key, "current_source_run_mismatch"),
        ] {
            if !matches {
                return Err(internal(problem));
            }
        }
        Ok(())
    }
}

/// Actual event eligibility never depends on the plan's claimed event.
fn actual_event_for(
    event_name: &str,
    payload: &serde_json::Value,
) -> Result<WorkflowEvent, OrchestratorError> {
    let event = workflow_event_for(event_name, payload)?;
    if event == WorkflowEvent::Local {
        return Err(internal("current_source_requires_runner_event"));
    }
    Ok(event)
}

/// Require an actual UTF-8 runner channel without silently falling back.
fn required_env(name: &str, problem: &str) -> Result<String, OrchestratorError> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| internal(problem))
}

/// Runner SHA is required even when the event payload already names a head.
fn validate_actual_sha(sha: Option<&str>) -> Result<(), OrchestratorError> {
    let sha = sha
        .filter(|sha| !sha.is_empty())
        .ok_or_else(|| internal("missing_actual_sha"))?;
    validate_diff_rev(sha, "bad_actual_sha").map_err(|problem| internal(&problem))
}

/// Identical PR parents cannot authorize a replacement merge commit.
fn verify_candidate_sha(sha: &str, candidate: &str) -> Result<(), OrchestratorError> {
    validate_actual_sha(Some(sha))?;
    if sha != candidate {
        return Err(internal("current_source_candidate_sha_mismatch"));
    }
    Ok(())
}

/// Event payload and runner environment must identify the same repository.
fn verify_payload_repository(
    payload: &serde_json::Value,
    repository: &str,
) -> Result<(), OrchestratorError> {
    let raw = payload["repository"]["full_name"]
        .as_str()
        .ok_or_else(|| internal("missing_actual_payload_repository"))?;
    let payload_repository = crate::origin::validate_repository_slug(raw)
        .ok_or_else(|| internal("bad_actual_payload_repository"))?;
    if payload_repository != repository {
        return Err(internal("current_source_payload_repository_mismatch"));
    }
    Ok(())
}

/// Require both immutable runner repository authority and matching Git origin.
fn current_repository(root: &Path) -> Result<String, OrchestratorError> {
    let raw = required_env("GITHUB_REPOSITORY", "missing_actual_repository")?;
    let repository = crate::origin::validate_repository_slug(&raw)
        .ok_or_else(|| internal("bad_actual_repository"))?;
    let origin = repository_slug_from_origin(root)
        .ok_or_else(|| internal("missing_current_source_origin"))?;
    if repository != origin {
        return Err(internal("current_source_repository_mismatch"));
    }
    Ok(repository)
}

#[cfg(test)]
#[path = "current_source_context_tests.rs"]
mod tests;
