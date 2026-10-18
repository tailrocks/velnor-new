//! Authenticated Qualification context from the current runner environment.

use std::env;
use std::path::PathBuf;

use velnor_actions_contract::workflow::{QualificationCacheProducerContext, QualificationRunRef};
use velnor_actions_contract::{
    QualificationDispatch, parse_strict_json, run_key_for_ci, validate_job_id,
};

use crate::OrchestratorError;
use crate::internal::internal;
use crate::prepare::GenerationPreparation;
use crate::request_event::{QualificationRunnerContext, qualification_dispatch_for_parts};

/// Actual runner facts used by the source-evidence writer.
pub(super) struct AuthenticatedProducerContext {
    /// Dispatch rebuilt from the current event payload and runner variables.
    pub(super) dispatch: QualificationDispatch,
    /// Sidecar form of the same authenticated runner context.
    pub(super) producer: QualificationCacheProducerContext,
    /// Current runner temporary root.
    pub(super) runner_temp: PathBuf,
    /// Current generated job identity from GitHub.
    pub(super) job_id: String,
    /// Canonical run key derived from the actual run and attempt.
    pub(super) run_key: String,
}

/// Rebuild Qualification provenance from the current runner variables.
///
/// # Errors
///
/// Gives an error when any required workflow value, event payload, or current
/// repository configuration is absent or differs from the source plan.
pub(super) fn read_authenticated_context(
    prepared: &GenerationPreparation,
) -> Result<AuthenticatedProducerContext, OrchestratorError> {
    let event_name = required("GITHUB_EVENT_NAME")?;
    if event_name != "workflow_dispatch" {
        return Err(internal("qualification_source_event_mismatch"));
    }
    let event_path = PathBuf::from(required_os("GITHUB_EVENT_PATH")?);
    let event_text =
        crate::safe_read::read_event_file(&event_path, crate::safe_read::MAX_REPO_FILE_BYTES)?;
    let event =
        parse_strict_json(&event_text).map_err(|_| internal("qualification_source_event_json"))?;

    let repository = required(crate::origin::GITHUB_REPOSITORY_ENV)?;
    let git_ref = required("GITHUB_REF")?;
    let ref_protected = parse_ref_protected(&required("GITHUB_REF_PROTECTED")?)?;
    let workflow_ref = required("GITHUB_WORKFLOW_REF")?;
    let workflow_sha = required("GITHUB_WORKFLOW_SHA")?;
    let source_sha = required("GITHUB_SHA")?;
    let run_id_text = required("GITHUB_RUN_ID")?;
    let run_attempt_text = required("GITHUB_RUN_ATTEMPT")?;
    let run_id = parse_positive_u64(&run_id_text, "run_id")?;
    let run_attempt = parse_positive_u32(&run_attempt_text, "run_attempt")?;
    let job_id = required("GITHUB_JOB")?;
    validate_job_id(&job_id).map_err(crate::internal::internal_contract)?;
    let runner_temp = PathBuf::from(required_os("RUNNER_TEMP")?);

    let mut dispatch = qualification_dispatch_for_parts(
        &event_name,
        &event,
        QualificationRunnerContext {
            repository: Some(&repository),
            git_ref: Some(&git_ref),
            ref_protected: Some(if ref_protected { "true" } else { "false" }),
            workflow_ref: Some(&workflow_ref),
            workflow_sha: Some(&workflow_sha),
            source_sha: Some(&source_sha),
            run_id: Some(&run_id_text),
            run_attempt: Some(&run_attempt_text),
        },
    )?
    .ok_or_else(|| internal("qualification_source_context_missing"))?;

    let image_source_sha = prepared
        .config
        .execution
        .as_ref()
        .and_then(|execution| execution.scale_set_image_source_sha())
        .map(str::to_owned);
    dispatch.scale_set_image_source_sha = image_source_sha;
    dispatch
        .validate_for_config(
            &prepared.default_branch,
            prepared.config.workflow.verification_branch.as_deref(),
            &repository,
            &source_sha,
        )
        .map_err(crate::internal::internal_contract)?;

    let run_key = run_key_for_ci(run_id, u64::from(run_attempt));
    let producer = QualificationCacheProducerContext {
        mode: dispatch.mode,
        repository: dispatch.repository.clone(),
        default_branch: dispatch.default_branch.clone(),
        git_ref: dispatch.git_ref.clone(),
        ref_protected: dispatch.ref_protected,
        workflow_ref: dispatch.workflow_ref.clone(),
        workflow_sha: dispatch.workflow_sha.clone(),
        source_sha: dispatch.source_sha.clone(),
        run: QualificationRunRef {
            run_id,
            run_attempt,
        },
    };
    Ok(AuthenticatedProducerContext {
        dispatch,
        producer,
        runner_temp,
        job_id,
        run_key,
    })
}

fn required(name: &str) -> Result<String, OrchestratorError> {
    env::var(name)
        .ok()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| internal(&format!("qualification_source_missing_{name}")))
}

fn required_os(name: &str) -> Result<std::ffi::OsString, OrchestratorError> {
    env::var_os(name)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| internal(&format!("qualification_source_missing_{name}")))
}

fn parse_ref_protected(value: &str) -> Result<bool, OrchestratorError> {
    match value {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(internal("qualification_source_ref_protected_invalid")),
    }
}

fn parse_positive_u64(value: &str, name: &str) -> Result<u64, OrchestratorError> {
    value
        .parse::<u64>()
        .ok()
        .filter(|parsed| *parsed > 0 && parsed.to_string() == value)
        .ok_or_else(|| internal(&format!("qualification_source_{name}_invalid")))
}

fn parse_positive_u32(value: &str, name: &str) -> Result<u32, OrchestratorError> {
    value
        .parse::<u32>()
        .ok()
        .filter(|parsed| *parsed > 0 && parsed.to_string() == value)
        .ok_or_else(|| internal(&format!("qualification_source_{name}_invalid")))
}

#[cfg(test)]
#[path = "qualification_source_evidence_context_tests.rs"]
mod tests;
