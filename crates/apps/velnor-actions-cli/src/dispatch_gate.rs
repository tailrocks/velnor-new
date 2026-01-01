//! Private file entrypoint gate: operation plus exact request-file path.
//!
//! Split from `dispatch` so that module keeps the 400-line gate. Reads
//! `VELNOR_INTERNAL_OP` plus the per-op environment and validates the
//! request-file presence rules; runners in `dispatch` execute the
//! validated request.

use std::env;
use std::path::{Path, PathBuf};

use velnor_actions_contract_workflow::{
    ARTIFACT_EXPORT_OPERATION, VERIFICATION_ARTIFACT_EXPORT_OPERATION,
};
use velnor_actions_orchestrator_baseline_publish::baseline_publish::PUBLISH_OP;
use velnor_actions_orchestrator_check_runtime::EXECUTE_CHECK_OP;
use velnor_actions_orchestrator_core::report_keys::REPORT_OP;
use velnor_actions_orchestrator_internal::internal::{
    MERGE_OP, PLAN_OP, REQUEST_FILE_ENV, WRITE_REQUEST_OP,
};
use velnor_actions_orchestrator_preseed_manifest::PRESEED_MANIFEST_OP;
use velnor_actions_orchestrator_retrieve_reports::FETCH_OP;

use crate::dispatch::RUNNER_TEMP_ENV;

/// Environment variable selecting the private operation. Never printed.
const OP_ENV: &str = "VELNOR_INTERNAL_OP";
/// Environment variable carrying the triggering event name. Never printed.
const GITHUB_EVENT_ENV: &str = "GITHUB_EVENT_NAME";
/// Environment variable carrying the event payload path. Never printed.
const GITHUB_EVENT_PATH_ENV: &str = "GITHUB_EVENT_PATH";

/// Private operation selected by the gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InternalOp {
    /// Materialize the request file from the GitHub environment.
    WriteRequest,
    ExecuteCheck,
    /// Plan operation.
    Plan,
    /// Merge operation.
    Merge,
    /// Matrix-report fetch operation.
    Fetch,
    /// Validate and stage a plan-declared artifact build result.
    ArtifactExport,
    /// Validate and stage a plan-declared verification task output.
    VerificationArtifactExport,
    /// Task-report production operation.
    Report,
    /// Pre-seed manifest-writing operation.
    PreseedManifest,
    /// Baseline-publish operation.
    Publish,
}

/// Validated private request: operation plus exact request-file path.
#[derive(Debug)]
pub(crate) struct InternalRequest {
    /// Operation to run.
    pub(crate) op: InternalOp,
    /// Exact request-file path from the environment.
    pub(crate) path: PathBuf,
}

/// Check the private gate: known op plus request-file presence by op.
///
/// Fetch and report take no request file: they need the runner-temp
/// velnor directory plus the numeric run ID instead. The manifest op
/// takes no request file either: runner temp scopes its output.
pub(crate) fn gate_request() -> Option<InternalRequest> {
    let op = match env::var(OP_ENV).as_deref() {
        Ok(tag) if tag == EXECUTE_CHECK_OP => InternalOp::ExecuteCheck,
        Ok(tag) if tag == WRITE_REQUEST_OP => InternalOp::WriteRequest,
        Ok(tag) if tag == PLAN_OP => InternalOp::Plan,
        Ok(tag) if tag == MERGE_OP => InternalOp::Merge,
        Ok(tag) if tag == FETCH_OP => InternalOp::Fetch,
        Ok(tag) if tag == ARTIFACT_EXPORT_OPERATION => InternalOp::ArtifactExport,
        Ok(tag) if tag == VERIFICATION_ARTIFACT_EXPORT_OPERATION => {
            InternalOp::VerificationArtifactExport
        }
        Ok(tag) if tag == REPORT_OP => InternalOp::Report,
        Ok(tag) if tag == PRESEED_MANIFEST_OP => InternalOp::PreseedManifest,
        Ok(tag) if tag == PUBLISH_OP => InternalOp::Publish,
        _ => return None,
    };
    if op == InternalOp::Fetch
        || op == InternalOp::Report
        || op == InternalOp::ExecuteCheck
        || op == InternalOp::ArtifactExport
        || op == InternalOp::VerificationArtifactExport
    {
        if env::var("GITHUB_RUN_ID").is_ok_and(|id| !id.is_empty()) {
            return runner_velnor_dir().map(|path| InternalRequest { op, path });
        }
        return None;
    }
    if op == InternalOp::PreseedManifest {
        return runner_velnor_dir().map(|path| InternalRequest { op, path });
    }
    let path = env::var_os(REQUEST_FILE_ENV)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)?;
    match op {
        InternalOp::WriteRequest => {
            if path.exists() {
                return None;
            }
            if !env::var(GITHUB_EVENT_ENV).is_ok_and(|name| !name.is_empty()) {
                return None;
            }
            if env::var_os(GITHUB_EVENT_PATH_ENV).is_none_or(|value| value.is_empty()) {
                return None;
            }
        }
        InternalOp::Plan | InternalOp::Merge | InternalOp::Publish => {
            if !path.is_file() {
                return None;
            }
        }
        InternalOp::Fetch
        | InternalOp::Report
        | InternalOp::ArtifactExport
        | InternalOp::VerificationArtifactExport
        | InternalOp::PreseedManifest
        | InternalOp::ExecuteCheck => {}
    }
    Some(InternalRequest { op, path })
}

/// Runner-temp velnor directory for file-less private operations.
///
/// `None` when `RUNNER_TEMP` is unset or empty; shared by the fetch,
/// report, and preseed-manifest gate branches so the scoping rule has
/// one definition.
fn runner_velnor_dir() -> Option<PathBuf> {
    env::var_os(RUNNER_TEMP_ENV)
        .filter(|value| !value.is_empty())
        .map(|temp| Path::new(&temp).join("velnor"))
}
