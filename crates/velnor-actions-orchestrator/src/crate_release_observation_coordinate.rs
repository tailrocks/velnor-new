// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    CrateRegistryRead, CrateReleaseAction, CrateReleaseAdmissionReceipt, CrateReleaseObservation,
    GitHubAdmissionRead, JournalIo, JournalMutationKind, JournalMutationPhase, OrchestratorError,
    ReconciledCandidate, RecoveryState, UploadAttempt, UploadPermit, derive_reconciled_candidate,
    invalid, metadata_action_matches, observe_crate_release, observe_prior_recovery,
    record_verified_mutation, repair_verified_metadata, verify_and_record_upload_intent,
    verify_observed_packages_after_recorded_upload,
};

pub(crate) fn coordinate_release<J, G, R>(
    input: &CrateReleaseObservation<'_, J, G, R>,
    mut verify_semver: impl FnMut(
        &str,
    ) -> Result<
        crate::crate_release_semver::VerifiedSemver,
        OrchestratorError,
    >,
    mut upload: impl FnMut(UploadPermit) -> Result<UploadAttempt, OrchestratorError>,
    mut wait: impl FnMut(),
) -> Result<ReconciledCandidate, OrchestratorError>
where
    J: JournalIo + crate::crate_release_journal::JournalOperationRefs,
    G: GitHubAdmissionRead + super::super::CrateReleaseRepairWrite,
    R: CrateRegistryRead,
{
    if input.sequence != 1 {
        return Err(invalid());
    }
    let (mut history, completed) = crate::crate_release_journal::inspect_recoverable_history(
        input.journal,
        input.identity,
        input.journal_receipt,
    )?;
    verify_predecessor(input)?;
    if completed.is_some() {
        return derive_reconciled_candidate(input);
    }
    let mut sequence = u32::try_from(history.len())
        .map_err(|_| invalid())?
        .checked_add(1)
        .ok_or_else(invalid)?;
    if history
        .last()
        .is_some_and(|item| matches!(item.phase, JournalMutationPhase::Intended))
    {
        record_verified_mutation(&CrateReleaseObservation { sequence, ..*input })?;
        sequence = sequence.checked_add(1).ok_or_else(invalid)?;
        history = crate::crate_release_journal::inspect_current_history(
            input.journal,
            input.identity,
            input.journal_receipt,
        )?;
        if usize::try_from(sequence - 1).map_err(|_| invalid())? != history.len() {
            return Err(invalid());
        }
    }
    for planned in &input.admission.packages {
        coordinate_package(
            input,
            planned,
            &mut sequence,
            &mut verify_semver,
            &mut upload,
            &mut wait,
        )?;
    }
    verify_predecessor(input)?;
    derive_reconciled_candidate(&CrateReleaseObservation { sequence, ..*input })
}

pub(super) fn coordinate_package<J, G, R>(
    input: &CrateReleaseObservation<'_, J, G, R>,
    planned: &super::super::CrateReleasePlannedPackage,
    sequence: &mut u32,
    verify_semver: &mut impl FnMut(
        &str,
    ) -> Result<
        crate::crate_release_semver::VerifiedSemver,
        OrchestratorError,
    >,
    upload: &mut impl FnMut(UploadPermit) -> Result<UploadAttempt, OrchestratorError>,
    wait: &mut impl FnMut(),
) -> Result<(), OrchestratorError>
where
    J: JournalIo + crate::crate_release_journal::JournalOperationRefs,
    G: GitHubAdmissionRead + super::super::CrateReleaseRepairWrite,
    R: CrateRegistryRead,
{
    let current = observe_crate_release(
        input.intent,
        input.request,
        input.github,
        input.registry,
        crate::CrateSource::new(input.source, input.git, input.source_execution),
    )?;
    verify_observed_packages_after_recorded_upload(
        &CrateReleaseObservation {
            sequence: *sequence,
            ..*input
        },
        &current,
    )?;
    let selected = current
        .packages
        .iter()
        .find(|item| item.source == planned.source)
        .ok_or_else(invalid)?;
    let package = &selected.source.name;
    if matches!(selected.action, CrateReleaseAction::Publish) {
        verify_predecessor(input)?;
        wait_for_predecessor_indexes(input, &current, package, wait)?;
        let semver = verify_semver(package)?;
        if *sequence >= 1000 {
            return Err(invalid());
        }
        let step = CrateReleaseObservation {
            sequence: *sequence,
            ..*input
        };
        let permit = verify_and_record_upload_intent(&step, package, semver)?;
        let _attempt = upload(permit)?;
        crate::crate_release_registry::wait_for_crate_version(
            input.registry,
            package,
            &selected.source.version,
            120,
            wait,
        )?;
        *sequence = sequence.checked_add(1).ok_or_else(invalid)?;
        record_verified_mutation(&CrateReleaseObservation {
            sequence: *sequence,
            ..*input
        })?;
        *sequence = sequence.checked_add(1).ok_or_else(invalid)?;
    }
    coordinate_metadata(input, planned, package, sequence)?;
    ensure_package_reconciled(
        &CrateReleaseObservation {
            sequence: *sequence,
            ..*input
        },
        planned,
    )
}

pub(super) fn coordinate_metadata<J, G, R>(
    input: &CrateReleaseObservation<'_, J, G, R>,
    planned: &super::super::CrateReleasePlannedPackage,
    package: &str,
    sequence: &mut u32,
) -> Result<(), OrchestratorError>
where
    J: JournalIo + crate::crate_release_journal::JournalOperationRefs,
    G: GitHubAdmissionRead + super::super::CrateReleaseRepairWrite,
    R: CrateRegistryRead,
{
    for kind in [
        JournalMutationKind::CreateTag,
        JournalMutationKind::CreateRelease,
    ] {
        let current = observe_crate_release(
            input.intent,
            input.request,
            input.github,
            input.registry,
            crate::CrateSource::new(input.source, input.git, input.source_execution),
        )?;
        verify_observed_packages_after_recorded_upload(
            &CrateReleaseObservation {
                sequence: *sequence,
                ..*input
            },
            &current,
        )?;
        let selected = current
            .packages
            .iter()
            .find(|item| item.source == planned.source)
            .ok_or_else(invalid)?;
        if metadata_action_matches(kind, &selected.action) {
            verify_predecessor(input)?;
            repair_verified_metadata(
                &CrateReleaseObservation {
                    sequence: *sequence,
                    ..*input
                },
                package,
                kind,
            )?;
            *sequence = sequence.checked_add(2).ok_or_else(invalid)?;
        }
    }
    Ok(())
}

pub(super) fn wait_for_predecessor_indexes<
    J: JournalIo,
    G: GitHubAdmissionRead,
    R: CrateRegistryRead,
>(
    input: &CrateReleaseObservation<'_, J, G, R>,
    current: &CrateReleaseAdmissionReceipt,
    package: &str,
    wait: &mut impl FnMut(),
) -> Result<(), OrchestratorError> {
    let effective = super::super::effective_release_intent(input.intent, input.request);
    let workspace = crate::crate_release_workspace::crate_release_workspace_plan_with_registry(
        input.source,
        &input.request.source_sha,
        &effective,
        input.registry,
        input.git,
        input.source_execution,
    )?;
    let predecessors: std::collections::BTreeSet<_> = workspace
        .plan
        .graph
        .edges
        .iter()
        .filter(|edge| edge.from == package)
        .map(|edge| edge.to.as_str())
        .collect();
    for name in predecessors {
        let dependency = current
            .packages
            .iter()
            .find(|item| item.source.name == name)
            .ok_or_else(invalid)?;
        let CrateReleaseAction::Reconciled { registry, .. } = &dependency.action else {
            return Err(invalid());
        };
        if registry.checksum != registry.downloaded_checksum {
            return Err(invalid());
        }
        crate::crate_release_registry::wait_for_crate_index(
            input.registry,
            name,
            &dependency.source.version,
            &registry.checksum,
            120,
            &mut *wait,
        )?;
    }
    Ok(())
}

pub(super) fn ensure_package_reconciled<
    J: JournalIo + crate::crate_release_journal::JournalOperationRefs,
    G: GitHubAdmissionRead,
    R: CrateRegistryRead,
>(
    input: &CrateReleaseObservation<'_, J, G, R>,
    planned: &super::super::CrateReleasePlannedPackage,
) -> Result<(), OrchestratorError> {
    let current = observe_crate_release(
        input.intent,
        input.request,
        input.github,
        input.registry,
        crate::CrateSource::new(input.source, input.git, input.source_execution),
    )?;
    verify_observed_packages_after_recorded_upload(input, &current)?;
    if !current.packages.iter().any(|item| {
        item.source == planned.source
            && matches!(item.action, CrateReleaseAction::Reconciled { .. })
    }) {
        return Err(invalid());
    }

    Ok(())
}

pub(super) fn verify_predecessor<J, G, R>(
    input: &CrateReleaseObservation<'_, J, G, R>,
) -> Result<(), OrchestratorError>
where
    J: JournalIo + crate::crate_release_journal::JournalOperationRefs,
    G: GitHubAdmissionRead,
    R: CrateRegistryRead,
{
    if let Some(previous) = &input.journal_receipt.predecessor {
        let observed = observe_prior_recovery(input, previous)?;
        if observed
            .iter()
            .any(|item| item.state != RecoveryState::Present)
        {
            return Err(invalid());
        }
    }
    Ok(())
}
