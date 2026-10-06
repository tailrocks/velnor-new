//! Fixed plan-request reader and source-binding checks.

use std::env;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use velnor_actions_contract::{QualificationDispatch, WorkflowEvent, parse_strict_json};

use crate::OrchestratorError;
use crate::internal::{check_schema, internal};
use crate::prepare::prepare;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct QualificationRequest {
    schema: u32,
    op: String,
    event: WorkflowEvent,
    #[serde(rename = "base", default)]
    _base: Option<String>,
    head: String,
    root: String,
    pub(super) repository: String,
    #[serde(rename = "qualification")]
    pub(super) context: QualificationDispatch,
}

impl QualificationRequest {
    pub(super) fn validate(&self, root: &Path) -> Result<(), OrchestratorError> {
        check_schema(self.schema)?;
        if self.op != "plan-v1" || self.event != WorkflowEvent::Qualification || self.root != "." {
            return Err(internal("qualification_request_mismatch"));
        }
        if self.repository != self.context.repository {
            return Err(internal("qualification_repository_mismatch"));
        }
        let prepared = prepare(root)?;
        self.context
            .validate_for(&prepared.default_branch, &self.repository, &self.head)
            .map_err(crate::internal::internal_contract)?;
        validate_runner_request(self, &self.context)
    }
}

pub(super) fn read_request(path: &Path) -> Result<QualificationRequest, OrchestratorError> {
    validate_request_path(path)?;
    let text = crate::safe_read::read_event_file(path, crate::safe_read::MAX_REPO_FILE_BYTES)?;
    let value = parse_strict_json(&text).map_err(crate::internal::internal_contract)?;
    serde_json::from_value(value).map_err(|_| internal("malformed_qualification_request"))
}

fn validate_request_path(path: &Path) -> Result<(), OrchestratorError> {
    let runner_temp = env::var_os("RUNNER_TEMP")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| internal("missing_runner_temp"))?;
    let expected = runner_temp.join("velnor/request/plan-request.json");
    if path != expected {
        return Err(internal("qualification_request_path_mismatch"));
    }
    Ok(())
}

fn validate_runner_request(
    request: &QualificationRequest,
    context: &QualificationDispatch,
) -> Result<(), OrchestratorError> {
    let event_name = env::var("GITHUB_EVENT_NAME").unwrap_or_default();
    if event_name != "workflow_dispatch" {
        return Err(internal("qualification_event_mismatch"));
    }
    let payload_path = env::var_os("GITHUB_EVENT_PATH")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| internal("missing_event_payload"))?;
    let payload =
        crate::safe_read::read_event_file(&payload_path, crate::safe_read::MAX_REPO_FILE_BYTES)?;
    let value: serde_json::Value =
        serde_json::from_str(&payload).map_err(|_| internal("malformed_event_payload"))?;
    let actual = crate::request_event::qualification_dispatch_for_parts(
        &event_name,
        &value,
        crate::request_event::QualificationRunnerContext {
            repository: env::var(crate::origin::GITHUB_REPOSITORY_ENV)
                .ok()
                .as_deref(),
            git_ref: env::var("GITHUB_REF").ok().as_deref(),
            ref_protected: env::var("GITHUB_REF_PROTECTED").ok().as_deref(),
            workflow_ref: env::var("GITHUB_WORKFLOW_REF").ok().as_deref(),
            workflow_sha: env::var("GITHUB_WORKFLOW_SHA").ok().as_deref(),
            source_sha: env::var("GITHUB_SHA").ok().as_deref(),
            run_id: env::var("GITHUB_RUN_ID").ok().as_deref(),
            run_attempt: env::var("GITHUB_RUN_ATTEMPT").ok().as_deref(),
        },
    )?;
    if actual.as_ref() != Some(context) || request.repository != context.repository {
        return Err(internal("qualification_runner_context_mismatch"));
    }
    Ok(())
}
