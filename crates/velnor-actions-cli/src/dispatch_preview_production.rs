//! Closed private preview operation dispatch; admission executes no source.

use std::process::ExitCode;
use velnor_actions_orchestrator::{admit_release_aggregate, execute_release_producer};

use super::operation::InternalOp;

/// Keep release and preview operation routing out of the private gate module.
pub(super) fn run(op: InternalOp) -> Option<ExitCode> {
    Some(match op {
        InternalOp::StableSourceSelection => {
            native(velnor_actions_orchestrator::select_stable_release_source_from_env())
        }
        InternalOp::ReleasePolicyAdmission => {
            native(velnor_actions_orchestrator::admit_release_policy_from_env())
        }
        InternalOp::PreviewSourceSelection => {
            native(velnor_actions_orchestrator::select_preview_source_from_env())
        }
        InternalOp::PreviewSourceFinalization => {
            native(velnor_actions_orchestrator::finalize_preview_source_from_env())
        }
        InternalOp::PreviewSourceAcquisition => {
            native(velnor_actions_orchestrator::acquire_preview_source_from_env())
        }
        InternalOp::StableTagIntent => {
            native(velnor_actions_orchestrator::write_stable_tag_intent_from_env())
        }
        InternalOp::StableTagOrigin => {
            native(velnor_actions_orchestrator::write_stable_tag_origin_from_env())
        }
        InternalOp::StableTagAdmission => {
            native(velnor_actions_orchestrator::admit_stable_tag_origin_from_env())
        }
        InternalOp::ExecuteReleaseProducer => produce(),
        InternalOp::AcquireReleaseLeaves => acquire(),
        InternalOp::AdmitReleaseAggregate => admit(),
        _ => return None,
    })
}

pub(super) fn produce() -> ExitCode {
    native(execute_release_producer())
}

pub(super) fn admit() -> ExitCode {
    native(admit_release_aggregate())
}

pub(super) fn native(
    value: Result<(), velnor_actions_orchestrator::OrchestratorError>,
) -> ExitCode {
    match value {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => super::fail_internal(&error.to_string()),
    }
}

/// Append only the canonical native digest; no credential is handed to source.
pub(super) fn acquire() -> ExitCode {
    use std::io::Write;
    let digest = match velnor_actions_orchestrator::acquire_release_leaves() {
        Ok(digest) => digest,
        Err(error) => return super::fail_internal(&error.to_string()),
    };
    let Some(path) = std::env::var_os("GITHUB_OUTPUT") else {
        return super::fail_internal("missing github output");
    };
    match std::fs::OpenOptions::new()
        .append(true)
        .open(path)
        .and_then(|mut file| writeln!(file, "admission_digest={digest}"))
    {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => super::fail_internal(&error.to_string()),
    }
}
