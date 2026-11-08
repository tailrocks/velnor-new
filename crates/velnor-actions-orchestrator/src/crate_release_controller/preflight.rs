// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    CrateRegistryRead, CrateReleaseAction, CrateReleaseAdmissionReceipt, GitHubAdmissionRead,
    OrchestratorError, PackageVerification, VerifiedReleasePackage, authorize_github_candidate,
    effective_release_intent, internal, observe_admitted_crate_release,
};

pub(crate) fn verify_release_package_with<G: GitHubAdmissionRead, R: CrateRegistryRead>(
    input: &PackageVerification<'_, G, R>,
    validate: impl Fn(&CrateReleaseAdmissionReceipt) -> Result<(), OrchestratorError>,
) -> Result<VerifiedReleasePackage, OrchestratorError> {
    verify_release_package_with_capacity(input, None, validate)
}

pub(crate) fn verify_release_package_with_capacity<G: GitHubAdmissionRead, R: CrateRegistryRead>(
    input: &PackageVerification<'_, G, R>,
    capacity: Option<&crate::crate_release_journal::CapacityReservation>,
    validate: impl Fn(&CrateReleaseAdmissionReceipt) -> Result<(), OrchestratorError>,
) -> Result<VerifiedReleasePackage, OrchestratorError> {
    let PackageVerification {
        receipt,
        intent,
        request,
        github,
        registry,
        source,
        git,
        package,
        source_execution,
    } = *input;
    if receipt.schema != 1 || receipt.intent != *intent || receipt.github.request != *request {
        return Err(internal("crate_release_receipt_mismatch"));
    }
    authorize_github_candidate(request, &receipt.github, github)?;
    let observed = observe_admitted_crate_release(
        intent,
        request,
        github,
        registry,
        crate::CrateSource::new(source, git, source_execution),
        &receipt.github,
    )?;
    validate(&observed)?;
    let index = observed
        .packages
        .iter()
        .position(|p| p.source.name == package)
        .ok_or_else(|| internal("crate_release_package_not_selected"))?;
    if observed.packages[..index]
        .iter()
        .any(|p| !p.action.is_reconciled())
        || observed.packages[index].action != CrateReleaseAction::Publish
    {
        return Err(internal("crate_release_package_not_ready"));
    }
    let target = tempfile::tempdir().map_err(|_| internal("crate_release_package_temp"))?;
    let effective_intent = effective_release_intent(intent, request);
    let archive = source_execution.package(&crate::CratePackageSource::new(
        source,
        &effective_intent,
        &observed.packages[index].source,
        target.path(),
        &request.source_sha,
        true,
        git,
    ))?;
    verify_archive(&archive, &observed.packages[index].source, capacity)?;
    if observe_admitted_crate_release(
        intent,
        request,
        github,
        registry,
        crate::CrateSource::new(source, git, source_execution),
        &receipt.github,
    )? != observed
    {
        return Err(internal("crate_release_package_state_changed"));
    }
    authorize_github_candidate(request, &receipt.github, github)?;
    let intent_bytes = velnor_actions_contract::canonical_json_bytes(intent)
        .map_err(|_| internal("crate_release_intent_invalid"))?;
    Ok(VerifiedReleasePackage {
        archive_sha256: crate::planner::generator::sha256_hex(&archive),
        intent_sha256: crate::planner::generator::sha256_hex(&intent_bytes),
        repository: request.repository.clone(),
        workflow_sha: request.workflow_sha.clone(),
        source_sha: request.source_sha.clone(),
        run_id: request.run_id,
        run_attempt: request.run_attempt,
        package: observed.packages[index].source.name.clone(),
        version: observed.packages[index].source.version.clone(),
    })
}

fn verify_archive(
    archive: &[u8],
    package: &crate::CrateReleaseSourcePackage,
    capacity: Option<&crate::crate_release_journal::CapacityReservation>,
) -> Result<(), OrchestratorError> {
    if let Some(capacity) = capacity {
        capacity.verify_archive(&package.name, &package.version, archive)?;
    }
    crate::release::crate_release_archive::crate_archive_features(
        archive,
        &package.name,
        &package.version,
    )?;
    Ok(())
}

pub(super) fn verify_package_binding(
    verified: &VerifiedReleasePackage,
    request: &crate::crate_release_github::GitHubAdmissionRequest,
    intent: &[u8],
) -> Result<(), OrchestratorError> {
    if verified.intent_sha256 != crate::planner::generator::sha256_hex(intent)
        || verified.repository != request.repository
        || verified.workflow_sha != request.workflow_sha
        || verified.source_sha != request.source_sha
        || verified.run_id != request.run_id
        || verified.run_attempt != request.run_attempt
    {
        return Err(internal("crate_release_verified_observation_invalid"));
    }
    Ok(())
}
