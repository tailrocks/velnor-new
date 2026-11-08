// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    CrateRegistryRead, CrateReleaseObservation, GitHubAdmissionRead, JournalIo, OrchestratorError,
    invalid,
};
use crate::crate_release_journal::{
    JournalEvidence, JournalOperationRefs, JournalReceipt, PriorHistorySnapshot,
};

pub(crate) struct FrozenCompletion {
    candidate: super::reconcile::ReconciledCandidate,
    admission: super::super::CrateReleaseAdmissionReceipt,
    receipt: JournalReceipt,
    predecessors: PriorHistorySnapshot,
    completed: Option<JournalEvidence>,
    tools: crate::release::package_release_signer::MeasuredReleaseTools,
    capacity: crate::crate_release_journal::CapacityReservation,
}

impl FrozenCompletion {
    #[cfg(test)]
    pub(crate) fn prepare_unprivileged<
        J: JournalIo + JournalOperationRefs,
        G: GitHubAdmissionRead,
        R: CrateRegistryRead,
    >(
        input: &CrateReleaseObservation<'_, J, G, R>,
        tools: crate::release::package_release_signer::MeasuredReleaseTools,
        execution: &crate::UnprivilegedSourceExecution,
    ) -> Result<Self, OrchestratorError> {
        let capacity = crate::crate_release_journal::CapacityReservation::prepare(
            input.journal_receipt,
            input.admission,
            input.intent,
            input.request,
            input.source,
            input.git,
        )?;
        Self::prepare_unprivileged_with_capacity(input, tools, execution, capacity)
    }

    pub(crate) fn prepare_unprivileged_with_capacity<
        J: JournalIo + JournalOperationRefs,
        G: GitHubAdmissionRead,
        R: CrateRegistryRead,
    >(
        input: &CrateReleaseObservation<'_, J, G, R>,
        tools: crate::release::package_release_signer::MeasuredReleaseTools,
        execution: &crate::UnprivilegedSourceExecution,
        capacity: crate::crate_release_journal::CapacityReservation,
    ) -> Result<Self, OrchestratorError> {
        Self::prepare(
            &CrateReleaseObservation {
                source_execution: execution,
                ..*input
            },
            tools,
            capacity,
        )
    }

    fn prepare<
        J: JournalIo + JournalOperationRefs,
        G: GitHubAdmissionRead,
        R: CrateRegistryRead,
    >(
        input: &CrateReleaseObservation<'_, J, G, R>,
        tools: crate::release::package_release_signer::MeasuredReleaseTools,
        capacity: crate::crate_release_journal::CapacityReservation,
    ) -> Result<Self, OrchestratorError> {
        let candidate = super::derive_reconciled_candidate(input)?;
        let deadline =
            velnor_actions_mise::CheckDeadline::after(std::time::Duration::from_secs(600))
                .map_err(|_| invalid())?
                .effective();
        let predecessors = snapshot_predecessors(input, deadline)?;
        let target = tempfile::tempdir().map_err(|_| invalid())?;
        for package in &candidate.packages {
            let archive = input
                .source_execution
                .package(&crate::CratePackageSource::new(
                    input.source,
                    input.intent,
                    &package.source,
                    target.path(),
                    &input.request.source_sha,
                    false,
                    input.git,
                ))?;
            capacity.verify_archive(&package.source.name, &package.source.version, &archive)?;
            if crate::release::crate_release_archive::crate_archive_inventory(
                &archive,
                &package.source.name,
                &package.source.version,
            )? != package.source_inventory
            {
                return Err(invalid());
            }
            if crate::crate_release_registry::reconcile_crate_registry(
                input.registry,
                &package.source.name,
                &package.source.version,
                &package.registry.owners,
                &archive,
            )? != package.registry
            {
                return Err(invalid());
            }
        }
        let (history, completed) = crate::crate_release_journal::inspect_recoverable_history(
            input.journal,
            input.identity,
            input.journal_receipt,
        )?;
        if history != candidate.history {
            return Err(invalid());
        }
        let frozen = Self {
            candidate,
            admission: input.admission.clone(),
            receipt: input.journal_receipt.clone(),
            predecessors,
            completed,
            tools,
            capacity,
        };
        frozen.revalidate_history(input.journal)?;
        crate::verify_journal_source(input.source, input.journal_receipt, input.git)?;
        Ok(frozen)
    }

    fn revalidate_history<J: JournalIo + JournalOperationRefs>(
        &self,
        journal: &J,
    ) -> Result<(), OrchestratorError> {
        if crate::crate_release_journal::inspect_recoverable_history(
            journal,
            &self.receipt.identity,
            &self.receipt,
        )? != (self.candidate.history.clone(), self.completed.clone())
        {
            return Err(invalid());
        }
        self.predecessors.revalidate(journal, &self.receipt)
    }

    pub(crate) fn record_unprivileged<
        J: JournalIo + JournalOperationRefs,
        G: GitHubAdmissionRead,
        R: CrateRegistryRead,
    >(
        self,
        _: &crate::UnprivilegedSourceExecution,
        journal: &J,
        github: &G,
        registry: &R,
    ) -> Result<String, OrchestratorError> {
        self.record(journal, github, registry, || Ok(()))
    }

    fn verify_packages<G: GitHubAdmissionRead, R: CrateRegistryRead>(
        &self,
        github: &G,
        registry: &R,
    ) -> Result<(), OrchestratorError> {
        let intent = &self.admission.intent;
        for package in &self.candidate.packages {
            let (owners, presence) = crate::crate_release_registry::inspect_crate_registry(
                registry,
                &package.source.name,
                &package.source.version,
            )?;
            if owners
                != (crate::crate_release_registry::CrateRegistryOwnership::Existing {
                    owners: package.registry.owners.clone(),
                })
                || presence != crate::crate_release_registry::CrateRegistryPresence::Present
            {
                return Err(invalid());
            }
            crate::crate_release_controller::require_reviewer_owner(
                registry,
                &package.registry.owners,
                intent.trusted_reviewer_id,
            )?;
            if crate::crate_release_github::read_github_tag_evidence(
                &self.receipt.identity.repository,
                &package.tag,
                github,
            )?
            .as_deref()
                != Some(&package.tag_commit)
                || crate::crate_release_github::read_github_release_evidence(
                    &self.receipt.identity.repository,
                    &package.tag,
                    &package.source.version,
                    github,
                )? != Some(crate::crate_release_github::GitHubReleaseEvidence {
                    id: package.release_id,
                    url: package.release_url.clone(),
                })
                || crate::crate_release_registry::revalidate_crate_registry(
                    registry,
                    &package.source.name,
                    &package.source.version,
                    &package.registry,
                )? != package.registry
            {
                return Err(invalid());
            }
            crate::crate_release_registry::wait_for_crate_index(
                registry,
                &package.source.name,
                &package.source.version,
                &package.registry.checksum,
                1,
                || {},
            )?;
        }
        Ok(())
    }

    fn record<J: JournalIo + JournalOperationRefs, G: GitHubAdmissionRead, R: CrateRegistryRead>(
        self,
        journal: &J,
        github: &G,
        registry: &R,
        before_write: impl Fn() -> Result<(), OrchestratorError>,
    ) -> Result<String, OrchestratorError> {
        before_write()?;
        let request = &self.admission.github.request;
        self.revalidate_history(journal)?;
        crate::crate_release_github::authorize_github_candidate(
            request,
            &self.admission.github,
            github,
        )?;
        crate::crate_release_github::verify_immutable_tag_rules(&request.repository, github)?;
        self.verify_packages(github, registry)?;
        crate::crate_release_github::authorize_github_candidate(
            request,
            &self.admission.github,
            github,
        )?;
        crate::crate_release_github::verify_immutable_tag_rules(&request.repository, github)?;
        self.revalidate_history(journal)?;
        let completion = crate::crate_release_journal::JournalCompletion {
            packages: super::reconciled_completion_packages(&self.candidate)?,
            post_publication_file_hashes: self.tools.into_pins(),
        };
        self.capacity.verify_completion(&completion)?;
        before_write()?;
        crate::crate_release_journal::record_frozen_completion(
            journal,
            ValidatedCompletion {
                receipt: self.receipt,
                completion,
                before_write: &before_write,
            },
        )
    }
}

pub(crate) struct ValidatedCompletion<'a> {
    receipt: JournalReceipt,
    completion: crate::crate_release_journal::JournalCompletion,
    before_write: &'a dyn Fn() -> Result<(), OrchestratorError>,
}

impl<'a> ValidatedCompletion<'a> {
    pub(crate) fn into_parts(
        self,
    ) -> (
        JournalReceipt,
        crate::crate_release_journal::JournalCompletion,
        &'a dyn Fn() -> Result<(), OrchestratorError>,
    ) {
        (self.receipt, self.completion, self.before_write)
    }
}

fn snapshot_predecessors<
    J: JournalIo + JournalOperationRefs,
    G: GitHubAdmissionRead,
    R: CrateRegistryRead,
>(
    input: &CrateReleaseObservation<'_, J, G, R>,
    deadline: velnor_actions_mise::CheckDeadline,
) -> Result<crate::crate_release_journal::PriorHistorySnapshot, OrchestratorError> {
    let context = input
        .journal_receipt
        .predecessor
        .as_ref()
        .map(|_| {
            crate::release::crate_release_direct_history::HistoryContext::admitted(
                input.source,
                input.intent,
                input.request,
                input.git,
                input.journal_receipt,
                deadline,
            )
        })
        .transpose()?;
    let edge = context
        .as_ref()
        .map(|context| crate::crate_release_journal::PriorTransition {
            context,
            github: input.github,
        });
    crate::crate_release_journal::walk_prior_history_until(
        input.journal,
        input.journal_receipt,
        input.source,
        input.git,
        edge.as_ref(),
        deadline,
        |_| Ok(()),
    )
}
