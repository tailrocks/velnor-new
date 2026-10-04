//! Fixed package-evidence and fresh reconciliation steps.
use std::collections::BTreeMap;

use velnor_actions_contract::{Step, config::RustReleaseConfig};
use velnor_actions_mise::{PinnedTool, ToolCatalog};
use velnor_actions_workflow_renderer::release_spec::{BootstrapPlan, ReleaseReconcilePolicy};

use super::JobInputs;
use crate::OrchestratorError;
pub(crate) use crate::release_emit::release_support_sources::ProofKind;

#[path = "release_proof.rs"]
mod proof;

/// Freeze configured approval before any job environment is assembled.
pub(crate) fn approved_policy(
    release: &RustReleaseConfig,
    plan: &BootstrapPlan,
    catalog: &ToolCatalog,
) -> Result<ReleaseReconcilePolicy, OrchestratorError> {
    let tags = plan
        .packages
        .iter()
        .map(|(name, version)| {
            let tag = release
                .tag_name
                .replace("{{ package }}", name)
                .replace("{{package}}", name)
                .replace("{{ version }}", version)
                .replace("{{version}}", version);
            (name.clone(), tag)
        })
        .collect();
    let tools = BTreeMap::from([
        ("generator".to_owned(), env!("CARGO_PKG_VERSION").to_owned()),
        (
            "release-plz".to_owned(),
            catalog.version(PinnedTool::ReleasePlz).to_owned(),
        ),
        (
            "rust".to_owned(),
            catalog.version(PinnedTool::Rust).to_owned(),
        ),
        (
            "python".to_owned(),
            catalog.version(PinnedTool::Python).to_owned(),
        ),
        ("gh".to_owned(), catalog.version(PinnedTool::Gh).to_owned()),
    ]);
    let policy = ReleaseReconcilePolicy {
        schema: 1,
        repository: plan.repository.clone(),
        registry: plan.registry.clone(),
        source_sha: plan.source_sha.clone(),
        packages: plan.packages.clone(),
        owners: release.expected_owners.clone(),
        tags,
        authentication: release.authentication,
        tools,
        intent_id: format!("release-{}", plan.source_sha),
    };
    policy.validate(plan)?;
    Ok(policy)
}

/// Build one fixed source-bound proof step for the named release operation.
pub(super) fn proof_step(
    inputs: &JobInputs<'_>,
    kind: ProofKind,
) -> Result<Step, OrchestratorError> {
    let record = proof_record(inputs, kind)?;
    velnor_actions_workflow_renderer::source_helper::source_helper_step(
        proof_step_name(kind),
        &record,
        record.environment().clone(),
    )
    .map_err(Into::into)
}

fn proof_step_name(kind: ProofKind) -> &'static str {
    match kind {
        ProofKind::SourceSnapshot => "Snapshot approved source",
        ProofKind::PreparedPackage => "Prepare original package archives",
        ProofKind::PackageVerify => "Verify original package archives",
        ProofKind::AnonymousPackage => "Create anonymous package evidence",
        ProofKind::PrepareAnonymous => "Prepare anonymous release proposal",
        ProofKind::PrepareForge => "Prepare forge release proposal",
        ProofKind::ForgePreflight => "Verify anonymous package evidence",
        ProofKind::RegistryArtifactProof => "Verify registry package artifact",
        ProofKind::RegistryPublishOidc | ProofKind::RegistryPublishBootstrap => {
            "Publish registry release"
        }
        ProofKind::ForgePublish => "Publish forge release",
        ProofKind::Reconcile => "Reconcile published package evidence",
    }
}

/// Build the immutable owner record used by the proof step and renderer registry.
///
/// The record owns its complete source, execution recipe, and environment. Callers
/// may clone it for registry admission, but cannot supply a replacement command.
/// # Errors
/// Fails closed when the Rust release SDK lacks a qualified host recipe or source.
pub(crate) fn proof_record(
    inputs: &JobInputs<'_>,
    kind: ProofKind,
) -> Result<velnor_actions_contract::CompiledSourceHelper, OrchestratorError> {
    proof::record(inputs, kind)
}

/// Exact qualified Full/Planning footprint of the source snapshot controller.
pub(crate) fn source_snapshot_tool_records(
    inputs: &JobInputs<'_>,
) -> Result<Vec<velnor_actions_contract::CompiledSourceHelper>, OrchestratorError> {
    proof::source_snapshot_tool_records(inputs)
}

/// Exact anonymous Full bootstrap and Python-only preparation footprint.
pub(crate) fn source_intent_control_tool_records(
    inputs: &JobInputs<'_>,
) -> Result<Vec<velnor_actions_contract::CompiledSourceHelper>, OrchestratorError> {
    proof::source_intent_control_tool_records(inputs)
}

/// Admit only the independently reconstructed Prepared producer at emission.
pub(crate) fn validate_prepared_workflow(
    inputs: &JobInputs<'_>,
    candidate: &velnor_actions_workflow_renderer::release_jobs::ReleaseWorkflowSpec,
) -> Result<(), OrchestratorError> {
    proof::validate_prepared_workflow(inputs, candidate)
}

/// Build the exact preparation record required before one proof helper.
///
/// The preparation source is scope independent. Anonymous scope is selected so
/// registry construction cannot accidentally admit a token-bearing preparation
/// record; the execution proof selects its own scope separately.
pub(crate) fn preparation_record(
    inputs: &JobInputs<'_>,
    kind: ProofKind,
) -> Result<velnor_actions_contract::CompiledSourceHelper, OrchestratorError> {
    proof::preparation_record(inputs, kind)
}
