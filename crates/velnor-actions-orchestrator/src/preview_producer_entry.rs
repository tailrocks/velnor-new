//! Closed workflow entrypoints for native payload production and union admission.
use super::PreviewProducerProof;
use crate::{
    OrchestratorError, exclusive_write::create_dir_no_symlink, internal::internal,
    prepare::prepare_with_config,
};
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use velnor_actions_contract::{
    PackageReleasePlan,
    config::{RustBinaryReleaseConfig, RustBinaryReleaseProducer},
};
use velnor_actions_mise::CheckDeadline;

#[path = "preview_production_acquisition.rs"]
mod acquisition;
pub use acquisition::{ACQUIRE_RELEASE_LEAVES_OP, acquire_release_leaves};
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
pub use admission::{ADMIT_RELEASE_AGGREGATE_OP, admit_release_aggregate};
#[cfg(test)]
use proof::publish_proof;
use proof::with_proof_ownership;

pub(super) fn release_workflow_context(
    plan: &PackageReleasePlan,
    deadline: CheckDeadline,
) -> Result<velnor_actions_contract::preview_production::ReleaseProductionContext, OrchestratorError>
{
    context::workflow_context(plan, deadline)
}

/// Private operation tag; its mandatory stage has no implicit serial fallback.
pub const EXECUTE_RELEASE_PRODUCER_OP: &str = "execute-release-producer-v1";

/// Execute one source-authorized payload or the complete admitted union.
/// # Errors
/// Rejects context, source, role, request, task, artifact or deadline drift.
pub fn execute_release_producer() -> Result<(), OrchestratorError> {
    acquisition::reject_credentials()?;
    let started = Instant::now();
    // Independent native producer ceiling until its configuration is read;
    // the configured scope shares this original start and can only shorten it.
    let maximum = producer_deadline(started, RustBinaryReleaseProducer::MAX_TIMEOUT_MINUTES)?;
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
                    .producer
                    .timeout_minutes;
                let deadline = producer_deadline(started, minutes)?;
                deadline
                    .scope(|| {
                        let (policy, plan) =
                            crate::release_policy_admission::admitted_policy_plan(&temp, deadline)?;
                        let preflight =
                            preflight_preview_source(&workspace, &temp, &config, &plan, deadline);
                        let prep = prepare_after_source_preflight(preflight, || {
                            prepare_with_config(&workspace, config)
                        })?;
                        let preview = source_preview(&prep, &policy, &plan)?;
                        let request = context::derive(&prep.root, &policy, plan, deadline)?;
                        execution::execute(&prep, &temp, &preview, &request, deadline, guard)
                    })
                    .map_err(|error| internal(&error.to_string()))?
            })
            .map_err(|error| internal(&error.to_string()))?
    })
}

/// Preserve the execution entry's original clock and reserve cleanup within it.
fn producer_deadline(started: Instant, minutes: u16) -> Result<CheckDeadline, OrchestratorError> {
    if !(1..=RustBinaryReleaseProducer::MAX_TIMEOUT_MINUTES).contains(&minutes) {
        return Err(internal("preview_producer_timeout"));
    }
    CheckDeadline::from_start_with_cleanup_reserve(
        started,
        Duration::from_secs(u64::from(minutes) * 60),
    )
    .map_err(|error| internal(&error.to_string()))
}

fn source_preview(
    prep: &crate::GenerationPreparation,
    policy: &velnor_actions_contract::PackageReleasePolicy,
    plan: &PackageReleasePlan,
) -> Result<RustBinaryReleaseConfig, OrchestratorError> {
    let release = prep
        .config
        .stacks
        .rust
        .as_ref()
        .map(|rust| &rust.binary_release)
        .ok_or_else(|| internal("preview_config_missing"))?;
    release.validate(".velnor/config.toml")?;
    let expected = crate::binary_preview_emit::release_policy(prep, release, plan.channel)?;
    if !release.channel_enabled(plan.channel) || policy != &expected {
        return Err(internal("preview_plan_config_mismatch"));
    }
    Ok(release.clone())
}

fn preflight_preview_source(
    workspace: &Path,
    temp: &Path,
    config: &velnor_actions_contract::config::VelnorConfig,
    plan: &PackageReleasePlan,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    if plan.channel != velnor_actions_contract::package_release::PackageReleaseChannel::Preview {
        return Ok(());
    }
    let release = config
        .stacks
        .rust
        .as_ref()
        .map(|rust| &rust.binary_release)
        .ok_or_else(|| internal("preview_config_missing"))?;
    release.validate(".velnor/config.toml")?;
    context::derive_context(workspace, plan, deadline)?;
    crate::release_policy_admission::validate_admitted_source_checkout(
        temp,
        workspace,
        release,
        &config.qualified_tools,
        deadline,
    )
}

fn prepare_after_source_preflight<T>(
    preflight: Result<(), OrchestratorError>,
    prepare: impl FnOnce() -> Result<T, OrchestratorError>,
) -> Result<T, OrchestratorError> {
    preflight?;
    prepare()
}

fn workflow_context() -> Result<(PathBuf, PathBuf), OrchestratorError> {
    if !is_release_event(&context::required_env("GITHUB_EVENT_NAME")?) {
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

fn is_release_event(event: &str) -> bool {
    matches!(event, "workflow_run" | "workflow_dispatch")
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
