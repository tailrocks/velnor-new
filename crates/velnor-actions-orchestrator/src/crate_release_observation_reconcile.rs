// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    CrateRegistryRead, CrateReleaseObservation, GitHubAdmissionRead, JournalIo, JournalMutation,
    JournalMutationPhase, JournalOwnerKind, OrchestratorError, authorize_github_candidate, invalid,
    observe_crate_release, verify_binding, verify_observed_packages_with_history,
    verify_predecessor,
};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReconciledCandidate {
    pub packages: Vec<super::super::CrateReleaseCompletedPackage>,
    pub history: Vec<JournalMutation>,
}

pub(crate) fn reconciled_completion_packages(
    candidate: &ReconciledCandidate,
) -> Result<Vec<crate::crate_release_journal::JournalPublishedPackage>, OrchestratorError> {
    use crate::crate_release_journal::{
        JournalFile, JournalFileKind, JournalOperation, JournalPublishedPackage,
    };
    candidate
        .packages
        .iter()
        .map(|package| {
            Ok(JournalPublishedPackage {
                name: package.source.name.clone(),
                version: package.source.version.clone(),
                owners: journal_owners(&package.registry.owners)?,
                registry_checksum: package.registry.checksum.clone(),
                downloaded_checksum: package.registry.downloaded_checksum.clone(),
                tag_target: package.tag_commit.clone(),
                release_id: package.release_id,
                release_url: package.release_url.clone(),
                files: package
                    .source_inventory
                    .iter()
                    .map(|(path, (mode, sha256))| JournalFile {
                        path: path.clone(),
                        sha256: sha256.clone(),
                        kind: JournalFileKind::Regular {
                            executable: mode & 0o111 != 0,
                        },
                    })
                    .collect(),
                features: package.registry.features.clone(),
                operations: vec![JournalOperation::ObservedPresent],
            })
        })
        .collect()
}

pub(super) fn journal_owners(
    owners: &[crate::crate_release_registry::CrateRegistryOwner],
) -> Result<Vec<crate::crate_release_journal::JournalOwner>, OrchestratorError> {
    owners
        .iter()
        .map(|owner| {
            let kind = match owner.kind.as_str() {
                "user" => JournalOwnerKind::User,
                "team" => JournalOwnerKind::Team,
                _ => return Err(invalid()),
            };
            Ok(crate::crate_release_journal::JournalOwner {
                id: owner.id,
                login: owner.login.clone(),
                kind,
            })
        })
        .collect()
}

pub(crate) fn derive_reconciled_candidate<
    J: JournalIo + crate::crate_release_journal::JournalOperationRefs,
    G: GitHubAdmissionRead,
    R: CrateRegistryRead,
>(
    input: &CrateReleaseObservation<'_, J, G, R>,
) -> Result<ReconciledCandidate, OrchestratorError> {
    verify_binding(input)?;
    verify_predecessor(input)?;
    authorize_github_candidate(input.request, &input.admission.github, input.github)?;
    let (history, completed) = crate::crate_release_journal::inspect_recoverable_history(
        input.journal,
        input.identity,
        input.journal_receipt,
    )?;
    if history
        .last()
        .is_some_and(|item| matches!(item.phase, JournalMutationPhase::Intended))
    {
        return Err(invalid());
    }
    let current = observe_crate_release(
        input.intent,
        input.request,
        input.github,
        input.registry,
        input.source,
        input.git,
    )?;
    let sequence = u32::try_from(history.len())
        .map_err(|_| invalid())?
        .checked_add(1)
        .ok_or_else(invalid)?;
    verify_observed_packages_with_history(
        &CrateReleaseObservation { sequence, ..*input },
        &current,
        None,
        true,
    )?;
    let packages = super::super::reconcile_crate_release(
        &current,
        input.intent,
        input.request,
        input.github,
        input.registry,
        input.source,
        input.git,
    )?;
    if packages.len() != input.admission.packages.len()
        || packages
            .iter()
            .zip(&input.admission.packages)
            .any(|(actual, planned)| actual.source != planned.source || actual.tag != planned.tag)
        || crate::crate_release_journal::inspect_recoverable_history(
            input.journal,
            input.identity,
            input.journal_receipt,
        )? != (history.clone(), completed)
    {
        return Err(invalid());
    }
    crate::verify_journal_source(input.source, input.journal_receipt, input.git)?;
    authorize_github_candidate(input.request, &input.admission.github, input.github)?;
    Ok(ReconciledCandidate { packages, history })
}
