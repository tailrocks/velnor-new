//! Protected hosted-run lookup and bounded qualification-receipt admission.

#[path = "qualification_resolver/api.rs"]
mod api;
#[path = "qualification_resolver/archive.rs"]
mod archive;
#[path = "qualification_resolver/delta.rs"]
mod delta;
#[path = "qualification_resolver/request.rs"]
mod request;
#[path = "qualification_resolver/wire.rs"]
mod wire;

use std::env;
use std::path::{Path, PathBuf};

use velnor_actions_contract::{
    QualificationCacheAdmission, QualificationPhase, canonical_json_bytes,
};
use velnor_actions_mise::ToolCatalog;

use crate::OrchestratorError;
use crate::internal::internal;
use crate::root::resolve_root;

/// Runner-temp staged plan-admission filename.
pub const QUALIFICATION_ADMISSION_FILENAME: &str = "qualification-cache-admission.json";
/// Private CLI operation tag for the plan-time receipt lookup.
pub const QUALIFICATION_RESOLVER_OP: &str = "resolve-qualification-v1";

/// Resolve and stage immutable predecessor evidence before planning.
/// # Errors
pub fn resolve_qualification_admission(request_path: &Path) -> Result<(), OrchestratorError> {
    let request = request::read_request(request_path)?;
    let root = checkout_root()?;
    request.validate(&root)?;
    let catalog = ToolCatalog::pinned();
    let client = api::GitHub::for_repository(&catalog, &request.repository)?;
    client.validate_current_branch(&request.context.default_branch)?;
    let Some(predecessor) = request.context.predecessor else {
        return Ok(());
    };
    let previous = api::resolve_chain(&client, predecessor, 1, None)?;
    let source_delta = if request.context.phase == QualificationPhase::UsefulDelta {
        let base = previous.receipt.source_sha.as_str();
        Some(delta::derive(&root, base, &request.context.source_sha)?)
    } else {
        None
    };
    let bytes = canonical_json_bytes(&wire::AdmissionDocument {
        predecessor: previous,
        source_delta,
    })
    .map_err(crate::internal::internal_contract)?;
    QualificationCacheAdmission::parse_bounded(&bytes)
        .map_err(crate::internal::internal_contract)?;
    stage_admission(&bytes)
}

/// Load the staged admission only for phases that require a predecessor.
/// # Errors
pub fn read_qualification_admission(
    response_json: &str,
    runner_temp: &Path,
) -> Result<Option<QualificationCacheAdmission>, OrchestratorError> {
    let response = crate::internal::PlanResponse::parse(response_json)?;
    let expected = response
        .plan
        .qualification
        .as_ref()
        .is_some_and(|context| context.phase.predecessor().is_some());
    let path = admission_path(runner_temp);
    if !expected {
        return match std::fs::symlink_metadata(&path) {
            Ok(_) => Err(internal("unexpected_qualification_admission")),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(_) => Err(internal("unreadable_qualification_admission")),
        };
    }
    let text = crate::safe_read::read_event_file(
        &path,
        velnor_actions_contract::MAX_QUALIFICATION_RECEIPT_BYTES as u64,
    )?;
    QualificationCacheAdmission::parse_bounded(text.as_bytes())
        .map(Some)
        .map_err(crate::internal::internal_contract)
}

fn checkout_root() -> Result<PathBuf, OrchestratorError> {
    let cwd = env::current_dir().map_err(|err| crate::OrchestratorError::RootDiscovery {
        problem: err.to_string(),
    })?;
    resolve_root(&cwd)
}

fn stage_admission(bytes: &[u8]) -> Result<(), OrchestratorError> {
    let runner_temp = env::var_os("RUNNER_TEMP")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| internal("missing_runner_temp"))?;
    let path = admission_path(&runner_temp);
    let parent = path
        .parent()
        .ok_or_else(|| internal("bad_qualification_admission_path"))?;
    crate::exclusive_write::create_dir_no_symlink(&runner_temp, parent)?;
    crate::exclusive_write::write_exclusive(&path, bytes, "qualification_admission")
}

fn admission_path(runner_temp: &Path) -> PathBuf {
    runner_temp
        .join("velnor")
        .join(QUALIFICATION_ADMISSION_FILENAME)
}
