// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    JournalAuthentication, JournalEvidence, JournalIdentity, JournalIo, JournalMutation,
    JournalMutationKind, JournalMutationPhase, JournalMutationResult, JournalOwnership,
    JournalReceipt, JournalStage, OrchestratorError, inspect_journal, invalid, read_mutation_at,
    valid_mutation, valid_receipt, valid_sha, validate_identity,
};

pub trait JournalOperationRefs {
    fn list_operation_refs(
        &self,
        identity: &JournalIdentity,
    ) -> Result<Vec<(String, String)>, OrchestratorError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PriorHistoryEvidence {
    pub start: JournalEvidence,
    pub mutations: Vec<JournalMutation>,
}

pub(crate) struct PriorTransition<'a> {
    pub(crate) source_execution: &'a dyn crate::CrateSourceExecution,
    pub(crate) request: &'a crate::crate_release_github::GitHubAdmissionRequest,
    pub(crate) github: &'a dyn crate::crate_release_github::GitHubAdmissionRead,
}

impl PriorTransition<'_> {
    fn verify(
        &self,
        prior: &JournalReceipt,
        current: &JournalReceipt,
    ) -> Result<(), OrchestratorError> {
        self.source_execution
            .historical_intent_reader()?
            .verify_transition(prior, current, self.request, self.github)
    }
}

pub(crate) fn inspect_current_history<I: JournalIo + JournalOperationRefs>(
    io: &I,
    identity: &JournalIdentity,
    receipt: &JournalReceipt,
) -> Result<Vec<JournalMutation>, OrchestratorError> {
    let (mutations, completed) = inspect_recoverable_history(io, identity, receipt)?;
    if completed.is_some()
        || mutations
            .last()
            .is_some_and(|item| matches!(item.phase, JournalMutationPhase::Intended))
    {
        return Err(invalid());
    }
    Ok(mutations)
}

pub(crate) fn inspect_recoverable_history<I: JournalIo + JournalOperationRefs>(
    io: &I,
    identity: &JournalIdentity,
    receipt: &JournalReceipt,
) -> Result<(Vec<JournalMutation>, Option<JournalEvidence>), OrchestratorError> {
    let start = inspect_journal(io, identity, JournalStage::Started)?.ok_or_else(invalid)?;
    let completed = inspect_journal(io, identity, JournalStage::Completed)?;
    if start.receipt != *receipt
        || completed
            .as_ref()
            .is_some_and(|value| value.receipt != *receipt)
    {
        return Err(invalid());
    }
    let listed = operation_sequences(io, identity)?;
    let mut mutations = Vec::with_capacity(listed.len());
    for (sequence, sha) in &listed {
        let record = read_mutation_at(io, identity, *sequence, sha)?;
        if record.receipt != *receipt
            || !valid_mutation(&record.mutation, receipt, mutations.last())
        {
            return Err(invalid());
        }
        mutations.push(record.mutation);
    }
    if (completed.is_some()
        && mutations
            .last()
            .is_some_and(|item| matches!(item.phase, JournalMutationPhase::Intended)))
        || operation_sequences(io, identity)? != listed
        || inspect_journal(io, identity, JournalStage::Started)?.as_ref() != Some(&start)
        || inspect_journal(io, identity, JournalStage::Completed)? != completed
    {
        return Err(invalid());
    }
    Ok((mutations, completed))
}

pub fn inspect_prior_history<I: JournalIo + JournalOperationRefs>(
    io: &I,
    previous: &JournalIdentity,
    current: &JournalReceipt,
    source: &std::path::Path,
    git: &crate::crate_release_git::ReleaseGit,
) -> Result<PriorHistoryEvidence, OrchestratorError> {
    inspect_prior_history_with(io, previous, current, source, git, None)
}

fn inspect_prior_history_with<I: JournalIo + JournalOperationRefs>(
    io: &I,
    previous: &JournalIdentity,
    current: &JournalReceipt,
    source: &std::path::Path,
    git: &crate::crate_release_git::ReleaseGit,
    edge: Option<&PriorTransition<'_>>,
) -> Result<PriorHistoryEvidence, OrchestratorError> {
    let start = inspect_prior_incomplete_inner(io, previous, current, source, true, git, edge)?;
    let listed = operation_sequences(io, previous)?;
    let mut mutations = Vec::with_capacity(listed.len());
    for (sequence, sha) in &listed {
        let record = read_mutation_at(io, previous, *sequence, sha)?;
        if record.receipt != start.receipt
            || !valid_mutation(&record.mutation, &start.receipt, mutations.last())
        {
            return Err(invalid());
        }
        mutations.push(record.mutation);
    }
    if operation_sequences(io, previous)? != listed
        || inspect_prior_incomplete_inner(io, previous, current, source, true, git, edge)? != start
    {
        return Err(invalid());
    }
    verify_new_crate_prior_intentions(&start.receipt, current, &mutations)?;
    Ok(PriorHistoryEvidence { start, mutations })
}

pub(super) fn operation_sequences<I: JournalOperationRefs>(
    io: &I,
    identity: &JournalIdentity,
) -> Result<Vec<(u32, String)>, OrchestratorError> {
    let prefix = format!(
        "refs/tags/velnor-receipt/{}/{}/operation-",
        identity.run_id, identity.run_attempt
    );
    let refs = io.list_operation_refs(identity)?;
    if refs.len() > 1000 {
        return Err(invalid());
    }
    let mut sequences = std::collections::BTreeMap::new();
    for (reference, sha) in refs {
        let value = reference.strip_prefix(&prefix).ok_or_else(invalid)?;
        let number = value.parse::<u32>().map_err(|_| invalid())?;
        if number == 0
            || number > 1000
            || number.to_string() != value
            || !valid_sha(&sha)
            || sequences.insert(number, sha).is_some()
        {
            return Err(invalid());
        }
    }
    let values: Vec<_> = sequences.into_iter().collect();
    if values
        .iter()
        .map(|(number, _)| *number)
        .ne(1..=u32::try_from(values.len()).map_err(|_| invalid())?)
    {
        return Err(invalid());
    }
    Ok(values)
}

pub fn inspect_prior_incomplete_evidence<I: JournalIo>(
    io: &I,
    previous: &JournalIdentity,
    current: &JournalReceipt,
    source: &std::path::Path,
    git: &crate::crate_release_git::ReleaseGit,
) -> Result<JournalEvidence, OrchestratorError> {
    inspect_prior_incomplete_inner(io, previous, current, source, false, git, None)
}

pub(super) fn inspect_prior_incomplete_inner<I: JournalIo>(
    io: &I,
    previous: &JournalIdentity,
    current: &JournalReceipt,
    source: &std::path::Path,
    allow_creation_transition: bool,
    git: &crate::crate_release_git::ReleaseGit,
    edge: Option<&PriorTransition<'_>>,
) -> Result<JournalEvidence, OrchestratorError> {
    validate_identity(&current.identity)?;
    if !valid_receipt(current, &current.identity)
        || current.predecessor.as_ref() != Some(previous)
        || (previous.run_id == current.identity.run_id
            && previous.run_attempt >= current.identity.run_attempt)
        || previous.repository != current.identity.repository
        || previous.source_sha != current.identity.source_sha
    {
        return Err(invalid());
    }
    crate::verify_journal_source(source, current, git)?;
    let prior = inspect_journal(io, previous, JournalStage::Started)?.ok_or_else(invalid)?;
    if inspect_journal(io, previous, JournalStage::Completed)?.is_some()
        || prior.receipt.registry != current.registry
        || prior.receipt.authentication != current.authentication
        || !prior_packages_compatible(&prior.receipt, current, allow_creation_transition)
        || prior.receipt.source_files != current.source_files
    {
        return Err(invalid());
    }
    if prior.receipt.base_intent_sha256 != current.base_intent_sha256 {
        edge.ok_or_else(invalid)?.verify(&prior.receipt, current)?;
    }
    Ok(prior)
}

pub(super) fn prior_packages_compatible(
    prior: &JournalReceipt,
    current: &JournalReceipt,
    allow_creation_transition: bool,
) -> bool {
    prior.packages.len() == current.packages.len() && prior.packages.iter().zip(&current.packages).all(|(old, now)| {
        if old.name != now.name || old.version != now.version || old.manifest != now.manifest || old.tag != now.tag { return false; }
        if old.owners == now.owners {
            if matches!(current.semver_evidence.get(&now.name), Some(crate::crate_release_semver::SemverEvidence::PriorNoBaselinePending { .. })) {
                return prior.predecessor.is_some() && prior.semver_evidence.get(&old.name) == current.semver_evidence.get(&now.name);
            }
            return true;
        }
        allow_creation_transition && prior.authentication == JournalAuthentication::BootstrapToken
            && matches!(old.owners, JournalOwnership::NewCrate)
            && matches!(now.owners, JournalOwnership::Existing { .. })
            && matches!(prior.semver_evidence.get(&old.name), Some(crate::crate_release_semver::SemverEvidence::NoBaseline { .. }))
            && matches!(current.semver_evidence.get(&now.name), Some(crate::crate_release_semver::SemverEvidence::PriorNoBaselinePending { .. }))
    })
}

pub(super) fn verify_new_crate_prior_intentions(
    prior: &JournalReceipt,
    current: &JournalReceipt,
    mutations: &[JournalMutation],
) -> Result<(), OrchestratorError> {
    use crate::crate_release_semver::SemverEvidence;
    for (old, now) in prior.packages.iter().zip(&current.packages) {
        if old.owners == now.owners {
            continue;
        }
        let Some(SemverEvidence::NoBaseline {
            checker_sha256: prior_checker,
        }) = prior.semver_evidence.get(&old.name)
        else {
            return Err(invalid());
        };
        let Some(SemverEvidence::PriorNoBaselinePending {
            checker_sha256,
            target_checksum,
        }) = current.semver_evidence.get(&now.name)
        else {
            return Err(invalid());
        };
        if prior_checker != checker_sha256 {
            return Err(invalid());
        }
        let candidates: Vec<_> = mutations
            .iter()
            .enumerate()
            .filter(|(_, item)| {
                item.package == old.name
                    && item.version == old.version
                    && item.kind == JournalMutationKind::Upload
                    && matches!(item.phase, JournalMutationPhase::Intended)
            })
            .collect();
        if candidates.len() != 1 {
            return Err(invalid());
        }
        let (index, intended) = candidates[0];
        if intended.semver_evidence != prior.semver_evidence.get(&old.name).cloned() {
            return Err(invalid());
        }
        if let Some(next) = mutations.get(index + 1) {
            let JournalMutationPhase::Observed {
                result:
                    JournalMutationResult::Upload {
                        registry_checksum,
                        downloaded_checksum,
                        owners,
                        ..
                    },
            } = &next.phase
            else {
                return Err(invalid());
            };
            let JournalOwnership::Existing { owners: actual } = &now.owners else {
                return Err(invalid());
            };
            if registry_checksum != target_checksum
                || downloaded_checksum != target_checksum
                || owners != actual
            {
                return Err(invalid());
            }
        }
    }
    Ok(())
}

#[cfg(test)]
pub(crate) fn inspect_prior_chain<I: JournalIo + JournalOperationRefs>(
    io: &I,
    current: &JournalReceipt,
    source: &std::path::Path,
    git: &crate::crate_release_git::ReleaseGit,
) -> Result<Vec<PriorHistoryEvidence>, OrchestratorError> {
    inspect_prior_chain_with(io, current, source, git, None)
}

pub(crate) fn inspect_prior_chain_with<I: JournalIo + JournalOperationRefs>(
    io: &I,
    current: &JournalReceipt,
    source: &std::path::Path,
    git: &crate::crate_release_git::ReleaseGit,
    edge: Option<&PriorTransition<'_>>,
) -> Result<Vec<PriorHistoryEvidence>, OrchestratorError> {
    let mut chain = Vec::new();
    let mut cursor = current.clone();
    let mut seen = std::collections::BTreeSet::new();
    seen.insert((cursor.identity.run_id, cursor.identity.run_attempt));
    while let Some(previous) = cursor.predecessor.clone() {
        if chain.len() == 32 || !seen.insert((previous.run_id, previous.run_attempt)) {
            return Err(invalid());
        }
        let evidence = inspect_prior_history_with(io, &previous, &cursor, source, git, edge)?;
        cursor = evidence.start.receipt.clone();
        chain.push(evidence);
    }
    Ok(chain)
}
