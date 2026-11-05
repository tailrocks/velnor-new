// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    CrateRegistryRead, CrateReleaseAction, CrateReleaseAdmissionReceipt, CrateReleaseObservation,
    GitHubAdmissionRead, JournalAuthentication, JournalFile, JournalFileKind, JournalIdentity,
    JournalIo, JournalMutation, JournalMutationKind, JournalMutationPhase, JournalMutationResult,
    JournalOwnerKind, JournalReceipt, OrchestratorError, invalid, journal_owners,
};

pub(crate) fn verify_binding<J: JournalIo, G: GitHubAdmissionRead, R: CrateRegistryRead>(
    input: &CrateReleaseObservation<'_, J, G, R>,
) -> Result<(), OrchestratorError> {
    let identity = input.identity;
    let request = input.request;
    let predecessor = input
        .intent
        .journal_predecessor
        .as_ref()
        .map(|previous| JournalIdentity {
            repository: request.repository.clone(),
            workflow_sha: previous.workflow_sha.clone(),
            source_sha: request.source_sha.clone(),
            run_id: previous.run_id,
            run_attempt: previous.run_attempt,
        });
    if input.journal_receipt.predecessor != predecessor
        || input.journal_receipt.base_intent_sha256
            != crate::crate_release_journal::release_base_intent_sha256(input.intent)?
    {
        return Err(invalid());
    }
    let authentication = journal_authentication(input.intent.authentication);
    if input.admission.schema != 1
        || input.admission.intent != *input.intent
        || input.admission.github.request != *request
        || input.journal_receipt.identity != *identity
        || input.journal_receipt.authentication != authentication
        || input.journal_receipt.semver_evidence != input.admission.semver_evidence
        || identity.repository != request.repository
        || identity.workflow_sha != request.workflow_sha
        || identity.source_sha != request.source_sha
        || identity.run_id != request.run_id
        || identity.run_attempt != request.run_attempt
        || input.journal_receipt.required_check_id != input.admission.github.required_check_id
        || input.journal_receipt.trusted_reviewer_id != request.trusted_reviewer_id
        || input.journal_receipt.workflow_run_url
            != format!(
                "https://github.com/{}/actions/runs/{}",
                request.repository, request.run_id
            )
    {
        return Err(invalid());
    }
    let intent_bytes =
        velnor_actions_contract::canonical_json_bytes(input.intent).map_err(|_| invalid())?;
    if input.journal_receipt.intent_sha256 != crate::generator::sha256_hex(&intent_bytes) {
        return Err(invalid());
    }
    crate::verify_journal_source(input.source, input.journal_receipt, input.git)?;
    verify_bound_packages(input.journal_receipt, input.admission)?;
    Ok(())
}

pub(super) fn verify_bound_packages(
    receipt: &JournalReceipt,
    admission: &CrateReleaseAdmissionReceipt,
) -> Result<(), OrchestratorError> {
    if receipt.packages.len() != admission.packages.len() {
        return Err(invalid());
    }
    for (recorded, admitted) in receipt.packages.iter().zip(&admission.packages) {
        if recorded.name != admitted.source.name
            || recorded.version != admitted.source.version
            || recorded.manifest != admitted.source.manifest
            || recorded.tag != admitted.tag
        {
            return Err(invalid());
        }
        match (&recorded.owners, &admitted.owners) {
            (
                crate::crate_release_journal::JournalOwnership::NewCrate,
                crate::crate_release_registry::CrateRegistryOwnership::NewCrate,
            ) => {}
            (
                crate::crate_release_journal::JournalOwnership::Existing { owners: left },
                crate::crate_release_registry::CrateRegistryOwnership::Existing { owners: right },
            ) if left.len() == right.len() => {
                for (left, right) in left.iter().zip(right) {
                    let kind = match left.kind {
                        JournalOwnerKind::User => "user",
                        JournalOwnerKind::Team => "team",
                    };
                    if left.id != right.id || left.login != right.login || kind != right.kind {
                        return Err(invalid());
                    }
                }
            }
            _ => return Err(invalid()),
        }
    }
    Ok(())
}

pub(super) fn journal_authentication(
    authentication: velnor_actions_contract::config::ReleaseAuthentication,
) -> JournalAuthentication {
    match authentication {
        velnor_actions_contract::config::ReleaseAuthentication::TrustedPublishing => {
            JournalAuthentication::TrustedPublishing
        }
        velnor_actions_contract::config::ReleaseAuthentication::BootstrapToken => {
            JournalAuthentication::BootstrapToken
        }
    }
}

pub(crate) fn verify_observed_packages_after_recorded_upload<
    J: JournalIo + crate::crate_release_journal::JournalOperationRefs,
    G,
    R,
>(
    input: &CrateReleaseObservation<'_, J, G, R>,
    current: &CrateReleaseAdmissionReceipt,
) -> Result<(), OrchestratorError> {
    verify_observed_packages_with_upload_intent(input, current, None)
}

pub(crate) fn verify_observed_packages_with_upload_intent<
    J: JournalIo + crate::crate_release_journal::JournalOperationRefs,
    G,
    R,
>(
    input: &CrateReleaseObservation<'_, J, G, R>,
    current: &CrateReleaseAdmissionReceipt,
    pending: Option<&JournalMutation>,
) -> Result<(), OrchestratorError> {
    verify_observed_packages_with_history(input, current, pending, false)
}

pub(super) fn verify_observed_packages_with_history<
    J: JournalIo + crate::crate_release_journal::JournalOperationRefs,
    G,
    R,
>(
    input: &CrateReleaseObservation<'_, J, G, R>,
    current: &CrateReleaseAdmissionReceipt,
    pending: Option<&JournalMutation>,
    allow_completed: bool,
) -> Result<(), OrchestratorError> {
    if verify_observed_packages(current, input.admission).is_ok() {
        return Ok(());
    }
    if current.intent != input.admission.intent
        || current.github != input.admission.github
        || current.packages.len() != input.admission.packages.len()
        || input.sequence <= 1
    {
        return Err(invalid());
    }
    let (records, completed) = crate::crate_release_journal::inspect_recoverable_history(
        input.journal,
        input.identity,
        input.journal_receipt,
    )?;
    if (completed.is_some() && !allow_completed)
        || records.len() != usize::try_from(input.sequence - 1).map_err(|_| invalid())?
    {
        return Err(invalid());
    }
    if pending.is_some_and(|item| {
        records.last() != Some(item)
            || item.kind != JournalMutationKind::Upload
            || !matches!(item.phase, JournalMutationPhase::Intended)
    }) {
        return Err(invalid());
    }
    for (now, old) in current.packages.iter().zip(&input.admission.packages) {
        if now.source != old.source || now.tag != old.tag {
            return Err(invalid());
        }
        if now.owners == old.owners {
            continue;
        }
        verify_creation_owner_transition(now, old, &records, pending)?;
    }
    Ok(())
}

pub(super) fn verify_creation_owner_transition(
    now: &super::super::CrateReleasePlannedPackage,
    old: &super::super::CrateReleasePlannedPackage,
    records: &[JournalMutation],
    pending: Option<&JournalMutation>,
) -> Result<(), OrchestratorError> {
    let crate::crate_release_registry::CrateRegistryOwnership::NewCrate = old.owners else {
        return Err(invalid());
    };
    let crate::crate_release_registry::CrateRegistryOwnership::Existing { owners } = &now.owners
    else {
        return Err(invalid());
    };
    let registry = match &now.action {
        CrateReleaseAction::RepairTag { registry }
        | CrateReleaseAction::RepairRelease { registry }
        | CrateReleaseAction::Reconciled { registry, .. } => registry,
        CrateReleaseAction::Publish => return Err(invalid()),
    };
    let pending_matches = pending.is_some_and(|item| {
        item.package == now.source.name
            && item.version == now.source.version
            && matches!(now.action, CrateReleaseAction::RepairTag { .. })
    });
    let found = pending_matches
        || records.iter().any(|record| {
            if record.package != now.source.name
                || record.version != now.source.version
                || record.kind != JournalMutationKind::Upload
            {
                return false;
            }
            let JournalMutationPhase::Observed {
                result:
                    JournalMutationResult::Upload {
                        registry_checksum,
                        downloaded_checksum,
                        owners: recorded,
                        ..
                    },
            } = &record.phase
            else {
                return false;
            };
            registry_checksum == &registry.checksum
                && downloaded_checksum == &registry.downloaded_checksum
                && recorded.len() == owners.len()
                && recorded.iter().zip(owners).all(|(left, right)| {
                    let kind = match left.kind {
                        JournalOwnerKind::User => "user",
                        JournalOwnerKind::Team => "team",
                    };
                    left.id == right.id && left.login == right.login && kind == right.kind
                })
        });
    if !found {
        return Err(invalid());
    }
    Ok(())
}

pub(crate) fn verify_observed_packages(
    current: &CrateReleaseAdmissionReceipt,
    prior: &CrateReleaseAdmissionReceipt,
) -> Result<(), OrchestratorError> {
    if current.intent != prior.intent
        || current.github != prior.github
        || current.packages.len() != prior.packages.len()
    {
        return Err(invalid());
    }
    if current
        .packages
        .iter()
        .zip(&prior.packages)
        .any(|(now, old)| {
            now.source != old.source || now.tag != old.tag || now.owners != old.owners
        })
    {
        return Err(invalid());
    }
    Ok(())
}

pub(super) fn derive_result<J: JournalIo, G: GitHubAdmissionRead, R: CrateRegistryRead>(
    input: &CrateReleaseObservation<'_, J, G, R>,
    package: &super::super::CrateReleasePlannedPackage,
    kind: JournalMutationKind,
) -> Result<JournalMutationResult, OrchestratorError> {
    match (kind, &package.action) {
        (
            JournalMutationKind::Upload,
            CrateReleaseAction::RepairTag { registry }
            | CrateReleaseAction::RepairRelease { registry }
            | CrateReleaseAction::Reconciled { registry, .. },
        ) => {
            let target = tempfile::tempdir().map_err(|_| invalid())?;
            let mut effective = input.intent.clone();
            if input.request.event == "push" {
                effective.recovery = None;
            }
            let archive = crate::CrateSourceExecution::package(
                input.source_execution,
                &crate::CratePackageSource::new(
                    input.source,
                    &effective,
                    &package.source,
                    target.path(),
                    &input.request.source_sha,
                    false,
                    input.git,
                ),
            )?;
            let files = crate::crate_release_archive::crate_archive_inventory(
                &archive,
                &package.source.name,
                &package.source.version,
            )?
            .into_iter()
            .map(|(path, (mode, sha256))| JournalFile {
                path,
                sha256,
                kind: JournalFileKind::Regular {
                    executable: mode != 0,
                },
            })
            .collect();
            let crate::crate_release_registry::CrateRegistryOwnership::Existing { owners } =
                &package.owners
            else {
                return Err(invalid());
            };
            let owners = journal_owners(owners)?;
            Ok(JournalMutationResult::Upload {
                registry_checksum: registry.checksum.clone(),
                downloaded_checksum: registry.downloaded_checksum.clone(),
                owners,
                features: registry.features.clone(),
                files,
            })
        }
        (
            JournalMutationKind::CreateTag,
            CrateReleaseAction::RepairRelease { .. } | CrateReleaseAction::Reconciled { .. },
        ) => Ok(JournalMutationResult::Tag {
            name: package.tag.clone(),
            target_sha: input.request.source_sha.clone(),
        }),
        (JournalMutationKind::CreateRelease, CrateReleaseAction::Reconciled { release, .. }) => {
            Ok(JournalMutationResult::Release {
                id: release.id,
                tag: package.tag.clone(),
                url: release.url.clone(),
            })
        }
        _ => Err(invalid()),
    }
}
