// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    CrateRegistryRead, CrateReleaseObservation, GitHubAdmissionRead, JournalIo, OrchestratorError,
    invalid,
};
use crate::crate_release_journal::{
    JournalEvidence, JournalIdentity, JournalOperationRefs, JournalReceipt, JournalStage,
    PriorHistoryEvidence,
};

pub(crate) struct FrozenCompletion {
    candidate: super::reconcile::ReconciledCandidate,
    admission: super::super::CrateReleaseAdmissionReceipt,
    receipt: JournalReceipt,
    predecessors: Vec<PriorHistoryEvidence>,
    completed: Option<JournalEvidence>,
    tools: crate::release::package_release_signer::MeasuredReleaseTools,
}

impl FrozenCompletion {
    pub(crate) fn prepare_unprivileged<
        J: JournalIo + JournalOperationRefs,
        G: GitHubAdmissionRead,
        R: CrateRegistryRead,
    >(
        input: &CrateReleaseObservation<'_, J, G, R>,
        tools: crate::release::package_release_signer::MeasuredReleaseTools,
        execution: &crate::UnprivilegedSourceExecution,
    ) -> Result<Self, OrchestratorError> {
        Self::prepare(
            &CrateReleaseObservation {
                source_execution: execution,
                ..*input
            },
            tools,
        )
    }

    fn prepare<
        J: JournalIo + JournalOperationRefs,
        G: GitHubAdmissionRead,
        R: CrateRegistryRead,
    >(
        input: &CrateReleaseObservation<'_, J, G, R>,
        tools: crate::release::package_release_signer::MeasuredReleaseTools,
    ) -> Result<Self, OrchestratorError> {
        let candidate = super::derive_reconciled_candidate(input)?;
        let predecessors = crate::crate_release_journal::inspect_prior_chain_with(
            input.journal,
            input.journal_receipt,
            input.source,
            input.git,
            Some(&crate::crate_release_journal::PriorTransition {
                request: input.request,
                github: input.github,
            }),
        )?;
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
        let mut next = self.receipt.predecessor.as_ref();
        for prior in &self.predecessors {
            let identity: &JournalIdentity = &prior.start.receipt.identity;
            if next != Some(identity)
                || crate::crate_release_journal::inspect_journal(
                    journal,
                    identity,
                    JournalStage::Started,
                )?
                .as_ref()
                    != Some(&prior.start)
                || crate::crate_release_journal::inspect_recoverable_history(
                    journal,
                    identity,
                    &prior.start.receipt,
                )? != (prior.mutations.clone(), None)
            {
                return Err(invalid());
            }
            next = prior.start.receipt.predecessor.as_ref();
        }
        if next.is_some() {
            return Err(invalid());
        }
        Ok(())
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
