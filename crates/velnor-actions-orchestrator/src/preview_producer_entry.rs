//! Closed workflow entrypoints for native payload production and union admission.
use super::PreviewProducerProof;
use crate::{
    OrchestratorError,
    exclusive_write::create_dir_no_symlink,
    internal::internal,
    prepare::prepare_with_config,
    retrieve_reports::{MAX_RETRIEVE_PLAN_BYTES, staged_reads::read_staged_bytes_until},
};
use std::{
    env,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use velnor_actions_contract::{
    PackageReleasePlan, config::RustBinaryPreviewConfig, package_release::PackageReleaseChannel,
    parse_package_release_plan,
};
use velnor_actions_mise::CheckDeadline;

#[path = "preview_production_acquisition.rs"]
mod acquisition;
pub use acquisition::{ACQUIRE_PREVIEW_LEAVES_OP, acquire_preview_leaves};
#[path = "preview_aggregate_admission.rs"]
mod admission;
#[path = "preview_aggregate.rs"]
mod aggregate;
#[path = "preview_production_context.rs"]
mod context;
#[path = "preview_production_execution.rs"]
mod execution;
#[path = "preview_production_handoff.rs"]
mod handoff;
#[path = "preview_production_leaf_manifest.rs"]
mod leaf_manifest;
#[path = "preview_producer_proof.rs"]
mod proof;
pub use admission::{ADMIT_PREVIEW_AGGREGATE_OP, admit_preview_aggregate};
#[cfg(test)]
use proof::publish_proof;
use proof::with_proof_ownership;

/// Private operation tag; its mandatory stage has no implicit serial fallback.
pub const EXECUTE_PREVIEW_PRODUCER_OP: &str = "execute-preview-producer-v1";
const PLAN_NAME: &str = "package-release-plan.json";

/// Execute one source-authorized payload or the complete admitted union.
/// # Errors
/// Rejects context, source, role, request, task, artifact or deadline drift.
pub fn execute_preview_producer() -> Result<(), OrchestratorError> {
    acquisition::reject_credentials()?;
    let started = Instant::now();
    // Absolute ceiling (GitHub's job limit) until the configured producer
    // budget is read; the nested scope below can only shorten it.
    let maximum = CheckDeadline::from_start_with_cleanup_reserve(
        started,
        Duration::from_secs(u64::from(velnor_actions_contract::JobTimeout::DEFAULT_MINUTES) * 60),
    )
    .map_err(|error| internal(&error.to_string()))?;
    with_proof_ownership(|guard| {
        maximum
            .scope(|| {
                let (workspace, temp) = workflow_context()?;
                let config = crate::config::load_config(&workspace)?;
                let minutes = config
                    .stacks
                    .rust
                    .as_ref()
                    .ok_or_else(|| internal("preview_config_missing"))?
                    .binary_release
                    .preview
                    .producer
                    .timeout_minutes;
                let deadline = CheckDeadline::from_start_with_cleanup_reserve(
                    started,
                    Duration::from_secs(u64::from(minutes) * 60),
                )
                .map_err(|error| internal(&error.to_string()))?;
                deadline
                    .scope(|| {
                        let plan = read_plan(&temp, deadline)?;
                        let prep = prepare_with_config(&workspace, config)?;
                        let preview = source_preview(&prep, &plan)?;
                        let request = context::derive(&prep.root, plan, deadline)?;
                        execution::execute(&prep, &temp, &preview, &request, deadline, guard)
                    })
                    .map_err(|error| internal(&error.to_string()))?
            })
            .map_err(|error| internal(&error.to_string()))?
    })
}

fn read_plan(
    temp: &Path,
    deadline: CheckDeadline,
) -> Result<PackageReleasePlan, OrchestratorError> {
    checkpoint(deadline)?;
    let path = env::var_os("VELNOR_REQUEST_FILE")
        .map(PathBuf::from)
        .ok_or_else(|| internal("preview_request_missing"))?;
    if path != temp.join(PLAN_NAME) {
        return Err(internal("preview_request_path_mismatch"));
    }
    let bytes =
        read_staged_bytes_until(&path, MAX_RETRIEVE_PLAN_BYTES, || read_checkpoint(deadline))
            .map_err(|_| internal("preview_plan_read"))?;
    let plan = parse_package_release_plan(&bytes).map_err(|_| internal("preview_plan_invalid"))?;
    if plan.channel != PackageReleaseChannel::Preview {
        return Err(internal("preview_plan_channel_mismatch"));
    }
    Ok(plan)
}

fn source_preview(
    prep: &crate::GenerationPreparation,
    plan: &PackageReleasePlan,
) -> Result<RustBinaryPreviewConfig, OrchestratorError> {
    let release = prep
        .config
        .stacks
        .rust
        .as_ref()
        .map(|rust| &rust.binary_release)
        .ok_or_else(|| internal("preview_config_missing"))?;
    let preview = &release.preview;
    preview.validate(".velnor/config.toml")?;
    if !preview.enabled || crate::binary_preview_emit::release_plan(prep, release)? != *plan {
        return Err(internal("preview_plan_config_mismatch"));
    }
    Ok(preview.clone())
}

fn workflow_context() -> Result<(PathBuf, PathBuf), OrchestratorError> {
    if !is_preview_event(&context::required_env("GITHUB_EVENT_NAME")?) {
        return Err(internal("preview_event_not_workflow_run"));
    }
    let workspace = PathBuf::from(context::required_env("GITHUB_WORKSPACE")?)
        .canonicalize()
        .map_err(|_| internal("preview_workspace"))?;
    let temp = PathBuf::from(context::required_env("RUNNER_TEMP")?)
        .canonicalize()
        .map_err(|_| internal("preview_runner_temp"))?;
    let root = temp.join("velnor");
    create_dir_no_symlink(&temp, &root)?;
    Ok((workspace, root))
}

fn is_preview_event(event: &str) -> bool {
    event == "workflow_run"
}
fn read_checkpoint(deadline: CheckDeadline) -> Result<(), &'static str> {
    deadline
        .remaining()
        .map(|_| ())
        .map_err(|_| "preview_deadline_exhausted")
}
fn checkpoint(deadline: CheckDeadline) -> Result<(), OrchestratorError> {
    read_checkpoint(deadline).map_err(internal)
}
#[cfg(test)]
#[path = "preview_producer_entry_tests.rs"]
mod tests;
