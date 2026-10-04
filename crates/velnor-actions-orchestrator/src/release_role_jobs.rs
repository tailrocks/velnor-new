//! Closed fresh-job assembly; credentials never share a runner with repository code.
use super::{JobInputs, ProofKind, preparation_step, proof_step};
use crate::{OrchestratorError, release_emit::release_checkouts::source_checkout};
use velnor_actions_contract::{JobTimeout, Step, ToolCacheDomain};
use velnor_actions_workflow_renderer::{
    mise_setup_step,
    release_artifact_channels::{job_outputs, upload_step},
    release_jobs::{ReleaseJobSpec, ReleaseRole},
    release_permissions::JobPermissions,
};

pub(super) fn job(
    inputs: &JobInputs<'_>,
    role: ReleaseRole,
) -> Result<ReleaseJobSpec, OrchestratorError> {
    if role == ReleaseRole::SourceSnapshotForge {
        return super::source_snapshot_job(inputs);
    }
    if role == ReleaseRole::PackagePreparedAnonymous {
        return super::prepared_package_job(inputs);
    }
    let condition = match role {
        ReleaseRole::RegistryPublishOidc
        | ReleaseRole::RegistryPublishBootstrap
        | ReleaseRole::ForgePublish
        | ReleaseRole::PreparationForge => Some(inputs.gate.clone()),
        ReleaseRole::Reconcile => Some(format!(
            "always() && needs.release-preflight.result == 'success' && ({})",
            inputs.gate
        )),
        _ => None,
    };
    let environment = matches!(
        role,
        ReleaseRole::RegistryPublishOidc
            | ReleaseRole::RegistryPublishBootstrap
            | ReleaseRole::ForgePublish
            | ReleaseRole::PreparationForge
    )
    .then(|| inputs.release.environment.clone());
    Ok(ReleaseJobSpec {
        role,
        display_name: role.job_id().to_owned(),
        runs_on: inputs.label.to_owned(),
        timeout_minutes: JobTimeout::RELEASE,
        needs: needs(role),
        condition,
        environment,
        permissions: JobPermissions::expected(role),
        steps: steps(inputs, role)?,
        outputs: job_outputs(role)?,
    })
}

fn needs(role: ReleaseRole) -> Vec<String> {
    let values: &[&str] = match role {
        ReleaseRole::PackagePreparedAnonymous => &["release-source-snapshot"],
        ReleaseRole::SourceSnapshotForge
        | ReleaseRole::PackageAnonymous
        | ReleaseRole::PreparationAnonymous => &[],
        ReleaseRole::PreflightForge => &["release-package"],
        ReleaseRole::RegistryPublishOidc | ReleaseRole::RegistryPublishBootstrap => {
            &["release-package", "release-preflight"]
        }
        ReleaseRole::ForgePublish => &[
            "release-package",
            "release-preflight",
            "release-registry-publish",
        ],
        ReleaseRole::Reconcile => &[
            "release-package",
            "release-preflight",
            "release-registry-publish",
            "release-forge-publish",
        ],
        ReleaseRole::PreparationForge => &["release-preparation-source"],
    };
    values.iter().map(ToString::to_string).collect()
}

fn steps(inputs: &JobInputs<'_>, role: ReleaseRole) -> Result<Vec<Step>, OrchestratorError> {
    let kind = kind(role);
    let anonymous = matches!(
        role,
        ReleaseRole::PackageAnonymous | ReleaseRole::PreparationAnonymous
    );
    let mut steps = Vec::new();
    if anonymous {
        steps.push(source_checkout(
            &inputs.bootstrap_tools.checkout_uses,
            inputs.sha,
        )?);
    }
    steps.push(mise_setup_step(
        &inputs.bootstrap_tools.mise,
        ToolCacheDomain::Full,
        inputs.label,
    )?);
    steps.push(preparation_step(
        inputs,
        if anonymous {
            kind
        } else {
            ProofKind::ForgePreflight
        },
    )?);
    if !anonymous {
        steps.push(super::admission_vectors::forge_admission(inputs)?);
    }
    if matches!(
        role,
        ReleaseRole::RegistryPublishOidc | ReleaseRole::RegistryPublishBootstrap
    ) {
        steps.push(proof_step(inputs, ProofKind::RegistryArtifactProof)?);
    }
    steps.push(proof_step(inputs, kind)?);
    steps.push(upload_step(role)?);
    Ok(steps)
}

fn kind(role: ReleaseRole) -> ProofKind {
    match role {
        ReleaseRole::SourceSnapshotForge => ProofKind::SourceSnapshot,
        ReleaseRole::PackageAnonymous => ProofKind::AnonymousPackage,
        ReleaseRole::PackagePreparedAnonymous => ProofKind::PreparedPackage,
        ReleaseRole::PreflightForge => ProofKind::ForgePreflight,
        ReleaseRole::RegistryPublishOidc => ProofKind::RegistryPublishOidc,
        ReleaseRole::RegistryPublishBootstrap => ProofKind::RegistryPublishBootstrap,
        ReleaseRole::ForgePublish => ProofKind::ForgePublish,
        ReleaseRole::Reconcile => ProofKind::Reconcile,
        ReleaseRole::PreparationAnonymous => ProofKind::PrepareAnonymous,
        ReleaseRole::PreparationForge => ProofKind::PrepareForge,
    }
}
