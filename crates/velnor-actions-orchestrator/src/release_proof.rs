//! Immutable source-bound Rust release proof records.

use std::collections::BTreeMap;

use velnor_actions_contract::{
    CompiledSourceHelper, HelperInvocation, SourceBoundHelper, SourceBoundOperation,
    workflow::native_tools::{CompiledNativeExecRecipe, NativeCredentialScope},
};
use velnor_actions_mise::catalog::{delivery_tools, qualification::DistributionHost};

use super::{JobInputs, ProofKind};
use crate::OrchestratorError;

const GENERATOR_VERSION: &str = env!("CARGO_PKG_VERSION");

#[path = "release_source_intent_records.rs"]
mod source_intent;

/// Exact anonymous Python-only control preparation footprint.
pub(super) fn source_intent_control_tool_records(
    inputs: &JobInputs<'_>,
) -> Result<Vec<CompiledSourceHelper>, OrchestratorError> {
    source_intent::tool_records(inputs)
}

/// Compare the final workflow's Prepared producer against fresh owner reconstruction.
pub(super) fn validate_prepared_workflow(
    inputs: &JobInputs<'_>,
    candidate: &velnor_actions_workflow_renderer::release_jobs::ReleaseWorkflowSpec,
) -> Result<(), OrchestratorError> {
    source_intent::prepared_input::validate_workflow(inputs, candidate)
}

/// Build one closed release proof owner record.
///
/// The source closure and the host-qualified Rust/Python/Gh execution envelope
/// come from their compiled owners. The proof source receives policy and event
/// identity through a fixed environment; it has no caller-selected argv.
/// # Errors
/// Fails closed when source compilation, environment construction, or SDK
/// qualification fails.
pub(super) fn record(
    inputs: &JobInputs<'_>,
    kind: ProofKind,
) -> Result<CompiledSourceHelper, OrchestratorError> {
    if kind == ProofKind::PreparedPackage {
        return source_intent::prepared_record(inputs);
    }
    if kind == ProofKind::PackageVerify {
        return source_intent::verified_record(inputs);
    }
    let operation = operation(kind);
    let source = crate::release_emit::release_support_sources::execution_source(
        inputs,
        GENERATOR_VERSION,
        kind,
    )?;
    let digest = velnor_actions_contract::compiled_source_sha256(source.as_bytes());
    let descriptor = SourceBoundHelper::compiled(operation, operation.path(), &digest)?;
    let recipe = execution_recipe(inputs, kind)?;
    let invocation = HelperInvocation::compiled(
        descriptor,
        Vec::new(),
        recipe.installed_selectors().to_vec(),
    )?;
    let environment = environment(inputs, kind, &recipe)?;
    let record = CompiledSourceHelper::compiled(invocation, source)?
        .with_environment(environment)
        .with_execution_recipe(recipe)?;
    if matches!(kind, ProofKind::SourceSnapshot) {
        record.with_github_output().map_err(Into::into)
    } else {
        Ok(record)
    }
}

/// Build the exact preparation record paired with the release execution recipe.
pub(super) fn preparation_record(
    inputs: &JobInputs<'_>,
    kind: ProofKind,
) -> Result<CompiledSourceHelper, OrchestratorError> {
    let host = release_host(inputs.label)?;
    match kind {
        ProofKind::SourceSnapshot => Err(contract(
            "source_snapshot_requires_scoped_tool_records".to_owned(),
        )),
        ProofKind::PreparedPackage | ProofKind::PackageVerify => {
            delivery_tools::source_intent_control_tools(host, GENERATOR_VERSION)
                .map(|tools| tools.preparation().clone())
                .map_err(|error| contract(format!("source_intent_control_tools:{error}")))
        }
        ProofKind::AnonymousPackage => delivery_tools::rust_release_tools(
            host,
            NativeCredentialScope::Anonymous,
            GENERATOR_VERSION,
        )
        .map(|tools| tools.preparation().clone())
        .map_err(|error| contract(format!("release_proof_tools:{error}"))),
        ProofKind::ForgePreflight
        | ProofKind::Reconcile
        | ProofKind::RegistryArtifactProof
        | ProofKind::RegistryPublishOidc
        | ProofKind::RegistryPublishBootstrap
        | ProofKind::ForgePublish
        | ProofKind::PrepareForge => delivery_tools::preparation(host, GENERATOR_VERSION)
            .map_err(|error| contract(format!("release_proof_tools:{error}"))),
        ProofKind::PrepareAnonymous => Err(contract(
            "release_proof_prepare_anonymous_unqualified".to_owned(),
        )),
    }
}

/// Exact two-domain bootstrap and preparation footprint for the source owner.
pub(super) fn source_snapshot_tool_records(
    inputs: &JobInputs<'_>,
) -> Result<Vec<CompiledSourceHelper>, OrchestratorError> {
    let tools =
        delivery_tools::source_snapshot_tools(release_host(inputs.label)?, GENERATOR_VERSION)
            .map_err(|error| contract(format!("source_snapshot_tools:{error}")))?;
    Ok(tools
        .bootstrap_records()
        .iter()
        .chain(tools.preparation_records())
        .cloned()
        .collect())
}

fn execution_recipe(
    inputs: &JobInputs<'_>,
    kind: ProofKind,
) -> Result<CompiledNativeExecRecipe, OrchestratorError> {
    let host = release_host(inputs.label)?;
    match kind {
        ProofKind::SourceSnapshot => delivery_tools::source_snapshot_tools(host, GENERATOR_VERSION)
            .map(|tools| tools.execution().clone())
            .map_err(|error| contract(format!("source_snapshot_tools:{error}"))),
        ProofKind::PreparedPackage | ProofKind::PackageVerify => {
            delivery_tools::source_intent_control_tools(host, GENERATOR_VERSION)
                .map(|tools| tools.execution().clone())
                .map_err(|error| contract(format!("source_intent_control_tools:{error}")))
        }
        ProofKind::AnonymousPackage => delivery_tools::rust_release_tools(
            host,
            NativeCredentialScope::Anonymous,
            GENERATOR_VERSION,
        )
        .map(|tools| tools.execution().clone())
        .map_err(|error| contract(format!("release_proof_tools:{error}"))),
        ProofKind::ForgePreflight | ProofKind::RegistryArtifactProof | ProofKind::Reconcile => {
            delivery_recipe(host, NativeCredentialScope::GithubReadOnly)
        }
        ProofKind::RegistryPublishOidc => {
            delivery_recipe(host, NativeCredentialScope::RustRegistryPublishOidc)
        }
        ProofKind::RegistryPublishBootstrap => {
            delivery_recipe(host, NativeCredentialScope::RustRegistryPublishBootstrap)
        }
        ProofKind::ForgePublish | ProofKind::PrepareForge => {
            delivery_recipe(host, NativeCredentialScope::GithubReleasePublish)
        }
        ProofKind::PrepareAnonymous => Err(contract(
            "release_proof_prepare_anonymous_unqualified".to_owned(),
        )),
    }
}

fn operation(kind: ProofKind) -> SourceBoundOperation {
    match kind {
        ProofKind::SourceSnapshot => SourceBoundOperation::RustReleaseSourceSnapshot,
        ProofKind::PreparedPackage => SourceBoundOperation::RustReleasePreparedPackage,
        ProofKind::PackageVerify => SourceBoundOperation::RustReleasePackageVerify,
        ProofKind::AnonymousPackage => SourceBoundOperation::RustReleaseAnonymousPackage,
        ProofKind::PrepareAnonymous => SourceBoundOperation::RustReleasePrepareAnonymous,
        ProofKind::PrepareForge => SourceBoundOperation::RustReleasePrepareForge,
        ProofKind::ForgePreflight => SourceBoundOperation::RustReleaseForgePreflight,
        ProofKind::RegistryArtifactProof => SourceBoundOperation::RustRegistryArtifactProof,
        ProofKind::RegistryPublishOidc | ProofKind::RegistryPublishBootstrap => {
            SourceBoundOperation::RustRegistryPublish
        }
        ProofKind::ForgePublish => SourceBoundOperation::RustForgePublish,
        ProofKind::Reconcile => SourceBoundOperation::RustReleaseReconcile,
    }
}

fn delivery_recipe(
    host: DistributionHost,
    scope: NativeCredentialScope,
) -> Result<CompiledNativeExecRecipe, OrchestratorError> {
    delivery_tools::execution_recipe(host, scope)
        .map_err(|error| contract(format!("release_proof_tools:{error}")))
}

fn release_host(label: &str) -> Result<DistributionHost, OrchestratorError> {
    match velnor_actions_contract::tool_target_for_runner_label(label) {
        Some("x86_64-unknown-linux-gnu") => Ok(DistributionHost::LinuxAmd64),
        Some("aarch64-unknown-linux-gnu") => Ok(DistributionHost::LinuxArm64),
        _ => Err(contract("release_proof_host".to_owned())),
    }
}

fn environment(
    inputs: &JobInputs<'_>,
    kind: ProofKind,
    recipe: &CompiledNativeExecRecipe,
) -> Result<BTreeMap<String, String>, OrchestratorError> {
    let mut environment = base_environment(inputs, recipe)?;
    kind_environment(inputs, kind, &mut environment)?;
    Ok(environment)
}

fn base_environment(
    inputs: &JobInputs<'_>,
    recipe: &CompiledNativeExecRecipe,
) -> Result<BTreeMap<String, String>, OrchestratorError> {
    let mut environment = recipe.environment().clone();
    environment.extend([
        (
            "GITHUB_WORKSPACE".to_owned(),
            "${{ github.workspace }}".to_owned(),
        ),
        ("GITHUB_SHA".to_owned(), "${{ github.sha }}".to_owned()),
        (
            "GITHUB_RUN_ID".to_owned(),
            "${{ github.run_id }}".to_owned(),
        ),
        (
            "GITHUB_RUN_ATTEMPT".to_owned(),
            "${{ github.run_attempt }}".to_owned(),
        ),
        (
            "RELEASE_RECONCILE_POLICY".to_owned(),
            inputs.reconciliation.serialized()?,
        ),
    ]);
    Ok(environment)
}

fn kind_environment(
    inputs: &JobInputs<'_>,
    kind: ProofKind,
    environment: &mut BTreeMap<String, String>,
) -> Result<(), OrchestratorError> {
    match kind {
        ProofKind::SourceSnapshot => {
            environment.insert(
                "RELEASE_DEFAULT_BRANCH".to_owned(),
                inputs.branch.to_owned(),
            );
        }
        ProofKind::AnonymousPackage => package_environment(inputs, environment)?,
        ProofKind::ForgePreflight
        | ProofKind::RegistryArtifactProof
        | ProofKind::RegistryPublishOidc
        | ProofKind::RegistryPublishBootstrap => package_artifact(environment),
        ProofKind::ForgePublish => forge_artifacts(environment),
        ProofKind::PrepareForge => prepare_forge_environment(inputs, environment),
        ProofKind::Reconcile => reconcile_artifacts(environment),
        ProofKind::PrepareAnonymous | ProofKind::PreparedPackage | ProofKind::PackageVerify => {}
    }
    Ok(())
}

fn package_artifact(environment: &mut BTreeMap<String, String>) {
    bind_artifact(
        environment,
        "RELEASE_PACKAGE_ARTIFACT",
        "release-package",
        "package-artifact",
    );
}

fn forge_artifacts(environment: &mut BTreeMap<String, String>) {
    package_artifact(environment);
    bind_artifact(
        environment,
        "RELEASE_REGISTRY_RECEIPT_ARTIFACT",
        "release-registry-publish",
        "registry-receipt-artifact",
    );
}

fn prepare_forge_environment(inputs: &JobInputs<'_>, environment: &mut BTreeMap<String, String>) {
    bind_artifact(
        environment,
        "RELEASE_PREPARE_ARTIFACT",
        "release-preparation-source",
        "artifact",
    );
    environment.extend([
        (
            "RELEASE_MANIFEST".to_owned(),
            inputs.release.manifest_path.clone(),
        ),
        (
            "RELEASE_DEFAULT_BRANCH".to_owned(),
            inputs.branch.to_owned(),
        ),
        (
            "GITHUB_ACTOR_ID".to_owned(),
            "${{ github.actor_id }}".to_owned(),
        ),
    ]);
}

fn reconcile_artifacts(environment: &mut BTreeMap<String, String>) {
    package_artifact(environment);
    bind_artifact(
        environment,
        "RELEASE_PREFLIGHT_ARTIFACT",
        "release-preflight",
        "preflight-artifact",
    );
    bind_artifact(
        environment,
        "RELEASE_REGISTRY_RECEIPT_ARTIFACT",
        "release-registry-publish",
        "registry-receipt-artifact",
    );
    bind_artifact(
        environment,
        "RELEASE_FORGE_RECEIPT_ARTIFACT",
        "release-forge-publish",
        "forge-receipt-artifact",
    );
}

fn package_environment(
    inputs: &JobInputs<'_>,
    environment: &mut BTreeMap<String, String>,
) -> Result<(), OrchestratorError> {
    environment.extend([
        (
            "RELEASE_EXPECTED_PACKAGES".to_owned(),
            serde_json::to_string(inputs.packages).map_err(|error| contract(error.to_string()))?,
        ),
        (
            "RELEASE_MANIFEST".to_owned(),
            inputs.release.manifest_path.clone(),
        ),
        ("RELEASE_REGISTRY".to_owned(), "crates-io".to_owned()),
        (
            "RELEASE_RUST_TOOLCHAIN".to_owned(),
            inputs
                .catalog
                .version(velnor_actions_mise::PinnedTool::Rust)
                .to_owned(),
        ),
        (
            "RELEASE_REPOSITORY".to_owned(),
            inputs.repository.to_owned(),
        ),
        (
            "RELEASE_PUBLISHABLE_WORKSPACE".to_owned(),
            if inputs.release.publishable_workspace {
                "1"
            } else {
                "0"
            }
            .to_owned(),
        ),
    ]);
    Ok(())
}

fn bind_artifact(
    environment: &mut BTreeMap<String, String>,
    environment_prefix: &str,
    producer_job: &str,
    output_prefix: &str,
) {
    environment.insert(
        format!("{environment_prefix}_ID"),
        format!("${{{{ needs.{producer_job}.outputs.{output_prefix}-id }}}}"),
    );
    environment.insert(
        format!("{environment_prefix}_DIGEST"),
        format!("${{{{ needs.{producer_job}.outputs.{output_prefix}-digest }}}}"),
    );
}

fn contract(problem: String) -> OrchestratorError {
    OrchestratorError::Contract { problem }
}
