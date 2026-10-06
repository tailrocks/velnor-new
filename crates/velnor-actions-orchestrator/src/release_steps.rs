//! Closed immutable release job assembly, with owner-frozen helper inventory.
use crate::OrchestratorError;
use std::collections::BTreeMap;
use velnor_actions_contract::Step;
use velnor_actions_contract::config::{ReleaseAuthentication, RustReleaseConfig};
use velnor_actions_mise::ToolCatalog;
use velnor_actions_workflow_renderer::release_jobs::{ReleaseJobSpec, ReleaseRole};

#[path = "release_admission_vectors.rs"]
mod admission_vectors;
pub(super) use admission_vectors::forge_admission_record;
#[path = "release_reconcile.rs"]
pub(crate) mod reconcile;
use reconcile::{ProofKind, proof_step};
#[path = "release_role_jobs.rs"]
mod role_jobs;
#[path = "release_source_job.rs"]
mod source_job;
pub(crate) use source_job::source_snapshot_job;
#[path = "release_prepared_job.rs"]
mod prepared_job;
pub(crate) use prepared_job::prepared_package_job;

/// Shared job inputs.
pub(crate) struct JobInputs<'a> {
    /// Original validated Rust selection, including intrinsic private package facts.
    pub(crate) selection: &'a velnor_actions_rust::release_select::ReleaseSelection,
    /// Validated release section.
    pub(crate) release: &'a RustReleaseConfig,
    /// Exact publish gate condition.
    pub(crate) gate: String,
    /// Approved source SHA every source checkout pins.
    pub(crate) sha: &'a str,
    /// Exact release origin.
    pub(crate) repository: &'a str,
    /// Default branch granting release authority.
    pub(crate) branch: &'a str,
    /// Approved package/version map frozen into the source plan.
    pub(crate) packages: &'a BTreeMap<String, String>,
    /// Frozen complete approval shared by proof helpers and renderer.
    pub(crate) reconciliation:
        &'a velnor_actions_workflow_renderer::release_spec::ReleaseReconcilePolicy,
    /// Actual registry identity, including the implicit default.
    pub(crate) actual_registry: &'a str,
    /// Literal runner label.
    pub(crate) label: &'a str,
    /// Pinned Mise setup step inputs.
    pub(crate) bootstrap_tools:
        &'a velnor_actions_workflow_renderer::release_bootstrap::ReleaseBootstrapApproval,
    /// Pinned tool catalog for coordinator argv.
    pub(crate) catalog: &'a ToolCatalog,
}

/// Build exactly the approved publisher mode and optional proposal pair.
pub(crate) fn assemble_jobs(
    inputs: &JobInputs<'_>,
) -> Result<BTreeMap<String, ReleaseJobSpec>, OrchestratorError> {
    let registry = match inputs.release.authentication {
        ReleaseAuthentication::TrustedPublishing => ReleaseRole::RegistryPublishOidc,
        ReleaseAuthentication::BootstrapToken => ReleaseRole::RegistryPublishBootstrap,
    };
    let mut roles = vec![
        ReleaseRole::PackageAnonymous,
        ReleaseRole::PreflightForge,
        registry,
        ReleaseRole::ForgePublish,
        ReleaseRole::Reconcile,
    ];
    if inputs.release.release_pr {
        roles.extend([
            ReleaseRole::PreparationAnonymous,
            ReleaseRole::PreparationForge,
        ]);
    }
    let mut jobs = BTreeMap::new();
    for role in roles {
        jobs.insert(role.job_id().to_owned(), role_jobs::job(inputs, role)?);
    }
    Ok(jobs)
}

/// Freeze the complete closed owner inventory in canonical order.
pub(crate) fn helper_registry(
    inputs: &JobInputs<'_>,
) -> Result<Vec<velnor_actions_contract::CompiledSourceHelper>, OrchestratorError> {
    let mut records = reconcile::source_snapshot_tool_records(inputs)?;
    for kind in crate::release_emit::release_support_sources::proof_record_kinds(
        inputs.release.authentication,
        inputs.release.release_pr,
    ) {
        records.push(reconcile::proof_record(inputs, kind)?);
    }
    for kind in crate::release_emit::release_support_sources::preparation_record_kinds(
        inputs.release.release_pr,
    ) {
        records.push(reconcile::preparation_record(inputs, kind)?);
    }
    records.push(forge_admission_record(inputs)?);
    Ok(records)
}

fn preparation_step(inputs: &JobInputs<'_>, kind: ProofKind) -> Result<Step, OrchestratorError> {
    let record = reconcile::preparation_record(inputs, kind)?;
    velnor_actions_workflow_renderer::source_helper::source_helper_step(
        "Prepare exact release tools",
        &record,
        record.environment().clone(),
    )
    .map_err(Into::into)
}
