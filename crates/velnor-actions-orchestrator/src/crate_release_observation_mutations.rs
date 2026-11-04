// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    CrateRegistryRead, CrateReleaseAction, CrateReleaseAdmissionReceipt, CrateReleaseObservation,
    GitHubAdmissionRead, JournalIntendedMutation, JournalIo, JournalMutation, JournalMutationKind,
    JournalMutationPhase, JournalStage, OrchestratorError, UploadPermit, VerifiedMutation,
    VerifiedReleasePackage, authorize_github_candidate, derive_result, inspect_journal,
    inspect_journal_mutation, invalid, observe_package_state, record_journal_intent,
    record_verified_observation, verify_binding, verify_observed_packages_after_recorded_upload,
    verify_observed_packages_with_upload_intent,
};

pub(crate) fn verify_and_record_upload_intent<
    J: JournalIo + crate::crate_release_journal::JournalOperationRefs,
    G: GitHubAdmissionRead,
    R: CrateRegistryRead,
>(
    input: &CrateReleaseObservation<'_, J, G, R>,
    package: &str,
    semver: crate::crate_release_semver::VerifiedSemver,
) -> Result<UploadPermit, OrchestratorError> {
    let verification = super::super::PackageVerification {
        receipt: input.admission,
        intent: input.intent,
        request: input.request,
        github: input.github,
        registry: input.registry,
        source: input.source,
        git: input.git,
        package,
    };
    let proof = super::super::verify_release_package_with(&verification, |observed| {
        verify_observed_packages_after_recorded_upload(input, observed)
    })?;
    record_admitted_upload_intent(input, proof, semver)
}

pub(super) fn bind_semver(
    evidence: crate::crate_release_semver::VerifiedSemver,
    package: &super::super::VerifiedReleasePackage,
    request: &crate::crate_release_github::GitHubAdmissionRequest,
    intent: &[u8],
) -> Result<crate::crate_release_semver::SemverEvidence, OrchestratorError> {
    evidence.into_bound_evidence(
        &package.package,
        &package.version,
        &request.source_sha,
        &crate::generator::sha256_hex(intent),
    )
}

pub(super) fn observe_input_state<J, G: GitHubAdmissionRead, R: CrateRegistryRead>(
    input: &CrateReleaseObservation<'_, J, G, R>,
) -> Result<CrateReleaseAdmissionReceipt, OrchestratorError> {
    observe_package_state(
        input.intent,
        input.request,
        input.github,
        input.registry,
        input.source,
        &input.admission.github,
        input.git,
    )
}

pub(crate) fn record_admitted_upload_intent<
    J: JournalIo + crate::crate_release_journal::JournalOperationRefs,
    G: GitHubAdmissionRead,
    R: CrateRegistryRead,
>(
    input: &CrateReleaseObservation<'_, J, G, R>,
    verified: VerifiedReleasePackage,
    semver: crate::crate_release_semver::VerifiedSemver,
) -> Result<UploadPermit, OrchestratorError> {
    verify_binding(input)?;
    let intent_bytes =
        velnor_actions_contract::canonical_json_bytes(input.intent).map_err(|_| invalid())?;
    let semver_evidence = bind_semver(semver, &verified, input.request, &intent_bytes)?;
    if verified.intent_sha256 != crate::generator::sha256_hex(&intent_bytes)
        || verified.repository != input.request.repository
        || verified.workflow_sha != input.request.workflow_sha
        || verified.source_sha != input.request.source_sha
        || verified.run_id != input.request.run_id
        || verified.run_attempt != input.request.run_attempt
    {
        return Err(invalid());
    }
    authorize_github_candidate(input.request, &input.admission.github, input.github)?;
    let started = inspect_journal(input.journal, input.identity, JournalStage::Started)?
        .ok_or_else(invalid)?;
    if started.receipt != *input.journal_receipt {
        return Err(invalid());
    }
    let state = observe_input_state(input)?;
    verify_observed_packages_after_recorded_upload(input, &state)?;
    let index = state
        .packages
        .iter()
        .position(|p| p.source.name == verified.package && p.source.version == verified.version)
        .ok_or_else(invalid)?;
    if state.packages[..index]
        .iter()
        .any(|p| !matches!(p.action, CrateReleaseAction::Reconciled { .. }))
    {
        return Err(invalid());
    }
    let selected = &state.packages[index];
    if selected.action != CrateReleaseAction::Publish {
        return Err(invalid());
    }
    if observe_input_state(input)? != state {
        return Err(invalid());
    }
    authorize_github_candidate(input.request, &input.admission.github, input.github)?;
    let intention_ref = record_journal_intent(
        input.journal,
        input.identity,
        input.journal_receipt,
        &JournalIntendedMutation {
            sequence: input.sequence,
            package: verified.package.clone(),
            version: verified.version.clone(),
            kind: JournalMutationKind::Upload,
            semver_evidence: Some(semver_evidence),
        },
    )?;
    Ok(UploadPermit {
        intent_sha256: verified.intent_sha256,
        repository: verified.repository,
        workflow_sha: verified.workflow_sha,
        source_sha: verified.source_sha,
        run_id: verified.run_id,
        run_attempt: verified.run_attempt,
        package: verified.package,
        version: verified.version,
        intention_ref,
        sequence: input.sequence,
    })
}

pub(crate) fn repair_verified_metadata<
    J: JournalIo + crate::crate_release_journal::JournalOperationRefs,
    G: GitHubAdmissionRead + super::super::CrateReleaseRepairWrite,
    R: CrateRegistryRead,
>(
    input: &CrateReleaseObservation<'_, J, G, R>,
    package: &str,
    kind: JournalMutationKind,
) -> Result<(), OrchestratorError> {
    let sequence = input
        .sequence
        .checked_add(1)
        .filter(|value| *value <= 1000)
        .ok_or_else(invalid)?;
    verify_binding(input)?;
    authorize_github_candidate(input.request, &input.admission.github, input.github)?;
    let started = inspect_journal(input.journal, input.identity, JournalStage::Started)?
        .ok_or_else(invalid)?;
    if started.receipt != *input.journal_receipt {
        return Err(invalid());
    }
    let state = observe_input_state(input)?;
    verify_observed_packages_after_recorded_upload(input, &state)?;
    let index = state
        .packages
        .iter()
        .position(|item| item.source.name == package)
        .ok_or_else(invalid)?;
    if state.packages[..index]
        .iter()
        .any(|item| !matches!(item.action, CrateReleaseAction::Reconciled { .. }))
    {
        return Err(invalid());
    }
    let selected = &state.packages[index];
    if !metadata_action_matches(kind, &selected.action) {
        return Err(invalid());
    }
    authorize_github_candidate(input.request, &input.admission.github, input.github)?;
    record_journal_intent(
        input.journal,
        input.identity,
        input.journal_receipt,
        &JournalIntendedMutation {
            sequence: input.sequence,
            package: selected.source.name.clone(),
            version: selected.source.version.clone(),
            kind,
            semver_evidence: None,
        },
    )?;
    if observe_input_state(input)? != state {
        return Err(invalid());
    }
    authorize_github_candidate(input.request, &input.admission.github, input.github)?;
    match kind {
        JournalMutationKind::CreateTag => input.github.create_tag(
            &input.intent.repository,
            &selected.tag,
            &input.request.source_sha,
        )?,
        JournalMutationKind::CreateRelease => input.github.create_release(
            &input.intent.repository,
            &selected.tag,
            &input.request.source_sha,
        )?,
        JournalMutationKind::Upload => return Err(invalid()),
    }
    record_verified_mutation(&CrateReleaseObservation { sequence, ..*input })?;
    Ok(())
}

pub(super) fn metadata_action_matches(
    kind: JournalMutationKind,
    action: &CrateReleaseAction,
) -> bool {
    matches!(
        (kind, action),
        (
            JournalMutationKind::CreateTag,
            CrateReleaseAction::RepairTag { .. }
        ) | (
            JournalMutationKind::CreateRelease,
            CrateReleaseAction::RepairRelease { .. }
        )
    )
}

pub(crate) fn record_verified_mutation<
    J: JournalIo + crate::crate_release_journal::JournalOperationRefs,
    G: GitHubAdmissionRead,
    R: CrateRegistryRead,
>(
    input: &CrateReleaseObservation<'_, J, G, R>,
) -> Result<String, OrchestratorError> {
    verify_binding(input)?;
    authorize_github_candidate(input.request, &input.admission.github, input.github)?;
    let started = inspect_journal(input.journal, input.identity, JournalStage::Started)?
        .ok_or_else(invalid)?;
    if started.receipt != *input.journal_receipt {
        return Err(invalid());
    }
    let previous_sequence = input
        .sequence
        .checked_sub(1)
        .filter(|seq| *seq > 0)
        .ok_or_else(invalid)?;
    let prior = inspect_journal_mutation(input.journal, input.identity, previous_sequence)?
        .ok_or_else(invalid)?;
    if !matches!(prior.phase, JournalMutationPhase::Intended) {
        return Err(invalid());
    }
    let state = observe_input_state(input)?;
    let pending_upload = (prior.kind == JournalMutationKind::Upload).then_some(&prior);
    verify_observed_packages_with_upload_intent(input, &state, pending_upload)?;
    let selected = state
        .packages
        .iter()
        .find(|package| {
            package.source.name == prior.package && package.source.version == prior.version
        })
        .ok_or_else(invalid)?;
    let result = derive_result(input, selected, prior.kind)?;
    if observe_input_state(input)? != state {
        return Err(invalid());
    }
    authorize_github_candidate(input.request, &input.admission.github, input.github)?;
    let mutation = JournalMutation {
        sequence: input.sequence,
        package: prior.package,
        version: prior.version,
        kind: prior.kind,
        semver_evidence: None,
        phase: JournalMutationPhase::Observed { result },
    };
    record_verified_observation(
        input.journal,
        input.identity,
        input.journal_receipt,
        VerifiedMutation(mutation),
    )
}
