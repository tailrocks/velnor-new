// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    CrateReleaseAction, CrateReleaseAdmissionReceipt, VerifiedReleasePackage,
    observe_admitted_crate_release, observe_package_state,
};
use crate::crate_release_github::{
    GitHubAdmissionRead, GitHubAdmissionRequest, authorize_github_candidate,
};
use crate::crate_release_journal::{
    JournalAuthentication, JournalFile, JournalFileKind, JournalIdentity, JournalIntendedMutation,
    JournalIo, JournalMutation, JournalMutationKind, JournalMutationPhase, JournalMutationResult,
    JournalOwnerKind, JournalReceipt, JournalStage, inspect_journal, inspect_journal_mutation,
    record_journal_intent, record_verified_observation,
};
use crate::crate_release_registry::CrateRegistryRead;
use crate::{OrchestratorError, internal::internal};
use std::path::Path;
use velnor_actions_contract::config::CrateReleaseIntent;

pub(crate) struct UploadPermit {
    pub(super) intent_sha256: String,
    pub(super) repository: String,
    pub(super) workflow_sha: String,
    pub(super) source_sha: String,
    pub(super) run_id: u64,
    pub(super) run_attempt: u32,
    pub(super) package: String,
    pub(super) version: String,
    pub(super) intention_ref: String,
    pub(super) sequence: u32,
}

pub(crate) struct VerifiedMutation(JournalMutation);

impl VerifiedMutation {
    pub(crate) fn into_inner(self) -> JournalMutation {
        self.0
    }
}

pub(crate) struct CrateReleaseObservation<'a, J, G, R> {
    pub journal: &'a J,
    pub identity: &'a JournalIdentity,
    pub journal_receipt: &'a JournalReceipt,
    pub admission: &'a CrateReleaseAdmissionReceipt,
    pub intent: &'a CrateReleaseIntent,
    pub request: &'a GitHubAdmissionRequest,
    pub github: &'a G,
    pub registry: &'a R,
    pub source: &'a Path,
    pub git: &'a crate::crate_release_git::ReleaseGit,
    pub source_execution: &'a dyn crate::CrateSourceExecution,
    pub sequence: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UploadAttempt {
    ExitedSuccess,
    Uncertain,
}

#[path = "crate_release_observation_coordinate.rs"]
mod coordinate;
#[path = "crate_release_observation_mutations.rs"]
mod mutations;
#[path = "crate_release_observation_reconcile.rs"]
mod reconcile;
#[path = "crate_release_observation_recovery.rs"]
mod recovery;
#[path = "crate_release_observation_validation.rs"]
mod validation;
#[cfg(test)]
pub(crate) use coordinate::coordinate_release;
pub(crate) use coordinate::coordinate_release_with_capacity;
use coordinate::verify_predecessor;
use mutations::metadata_action_matches;
pub(crate) use mutations::{
    record_verified_mutation, repair_verified_metadata, verify_and_record_upload_intent,
};
use reconcile::journal_owners;
pub(crate) use reconcile::{
    ReconciledCandidate, derive_reconciled_candidate, reconciled_completion_packages,
};
pub(crate) use recovery::{RecoveryState, observe_prior_recovery};
pub(crate) use validation::verify_observed_packages;
use validation::{derive_result, verify_observed_packages_with_history};
pub(super) use validation::{
    verify_binding, verify_observed_packages_after_recorded_upload,
    verify_observed_packages_with_upload_intent,
};

fn invalid() -> OrchestratorError {
    internal("crate_release_verified_observation_invalid")
}

fn verify_sequence_room<J, G, R>(
    input: &CrateReleaseObservation<'_, J, G, R>,
    history: &[JournalMutation],
) -> Result<(), OrchestratorError>
where
    J: JournalIo + crate::crate_release_journal::JournalOperationRefs,
    G: GitHubAdmissionRead,
    R: CrateRegistryRead,
{
    let sequence = u32::try_from(history.len())
        .map_err(|_| invalid())?
        .checked_add(1)
        .ok_or_else(invalid)?;
    let step = CrateReleaseObservation { sequence, ..*input };
    let pending = history
        .last()
        .filter(|item| matches!(item.phase, JournalMutationPhase::Intended));
    let state = mutations::observe_input_state(&step)?;
    verify_observed_packages_with_upload_intent(
        &step,
        &state,
        pending.filter(|item| item.kind == JournalMutationKind::Upload),
    )?;
    crate::crate_release_journal::verify_sequence_capacity(&state, history.len(), pending.is_some())
}

fn verify_standalone_sequence_room<J, G, R>(
    input: &CrateReleaseObservation<'_, J, G, R>,
) -> Result<(), OrchestratorError>
where
    J: JournalIo + crate::crate_release_journal::JournalOperationRefs,
    G: GitHubAdmissionRead,
    R: CrateRegistryRead,
{
    let (history, completed) = crate::crate_release_journal::inspect_recoverable_history(
        input.journal,
        input.identity,
        input.journal_receipt,
    )?;
    if completed.is_some() {
        return Err(invalid());
    }
    verify_sequence_room(input, &history)
}

impl<J, G, R> std::fmt::Debug for CrateReleaseObservation<'_, J, G, R> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CrateReleaseObservation")
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
pub(super) use mutations::record_admitted_upload_intent;

#[path = "crate_release_observation_completion.rs"]
mod completion;
pub(crate) use completion::{FrozenCompletion, ValidatedCompletion};
