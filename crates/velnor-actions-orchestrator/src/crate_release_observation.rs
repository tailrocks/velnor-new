// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    CrateReleaseAction, CrateReleaseAdmissionReceipt, VerifiedReleasePackage,
    observe_crate_release, observe_package_state, package_source,
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
    pub git: &'a velnor_actions_mise::release_git_pin::VerifiedGit,
    pub sequence: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UploadAttempt {
    ExitedSuccess,
    Uncertain,
}

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
    G: GitHubAdmissionRead + super::CrateReleaseRepairWrite,
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

fn coordinate_package<J, G, R>(
    input: &CrateReleaseObservation<'_, J, G, R>,
    planned: &super::CrateReleasePlannedPackage,
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
    G: GitHubAdmissionRead + super::CrateReleaseRepairWrite,
    R: CrateRegistryRead,
{
    let current = observe_crate_release(
        input.intent,
        input.request,
        input.github,
        input.registry,
        input.source,
        input.git,
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

fn coordinate_metadata<J, G, R>(
    input: &CrateReleaseObservation<'_, J, G, R>,
    planned: &super::CrateReleasePlannedPackage,
    package: &str,
    sequence: &mut u32,
) -> Result<(), OrchestratorError>
where
    J: JournalIo + crate::crate_release_journal::JournalOperationRefs,
    G: GitHubAdmissionRead + super::CrateReleaseRepairWrite,
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
            input.source,
            input.git,
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

fn wait_for_predecessor_indexes<J: JournalIo, G: GitHubAdmissionRead, R: CrateRegistryRead>(
    input: &CrateReleaseObservation<'_, J, G, R>,
    current: &CrateReleaseAdmissionReceipt,
    package: &str,
    wait: &mut impl FnMut(),
) -> Result<(), OrchestratorError> {
    let effective = super::effective_release_intent(input.intent, input.request);
    let workspace = crate::crate_release_workspace::crate_release_workspace_plan_with_registry(
        input.source,
        &input.request.source_sha,
        &effective,
        input.registry,
        input.git,
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

fn ensure_package_reconciled<
    J: JournalIo + crate::crate_release_journal::JournalOperationRefs,
    G: GitHubAdmissionRead,
    R: CrateRegistryRead,
>(
    input: &CrateReleaseObservation<'_, J, G, R>,
    planned: &super::CrateReleasePlannedPackage,
) -> Result<(), OrchestratorError> {
    let current = observe_crate_release(
        input.intent,
        input.request,
        input.github,
        input.registry,
        input.source,
        input.git,
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

fn verify_predecessor<J, G, R>(
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

pub(crate) fn verify_and_record_upload_intent<
    J: JournalIo + crate::crate_release_journal::JournalOperationRefs,
    G: GitHubAdmissionRead,
    R: CrateRegistryRead,
>(
    input: &CrateReleaseObservation<'_, J, G, R>,
    package: &str,
    semver: crate::crate_release_semver::VerifiedSemver,
) -> Result<UploadPermit, OrchestratorError> {
    let verification = super::PackageVerification {
        receipt: input.admission,
        intent: input.intent,
        request: input.request,
        github: input.github,
        registry: input.registry,
        source: input.source,
        git: input.git,
        package,
    };
    let proof = super::verify_release_package_with(&verification, |observed| {
        verify_observed_packages_after_recorded_upload(input, observed)
    })?;
    record_admitted_upload_intent(input, proof, semver)
}

fn bind_semver(
    evidence: crate::crate_release_semver::VerifiedSemver,
    package: &super::VerifiedReleasePackage,
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

pub(super) fn record_admitted_upload_intent<
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
    let state = observe_package_state(
        input.intent,
        input.request,
        input.github,
        input.registry,
        input.source,
        &input.admission.github,
        input.git,
    )?;
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
    if observe_package_state(
        input.intent,
        input.request,
        input.github,
        input.registry,
        input.source,
        &input.admission.github,
        input.git,
    )? != state
    {
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
    G: GitHubAdmissionRead + super::CrateReleaseRepairWrite,
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
    let state = observe_package_state(
        input.intent,
        input.request,
        input.github,
        input.registry,
        input.source,
        &input.admission.github,
        input.git,
    )?;
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
    if observe_package_state(
        input.intent,
        input.request,
        input.github,
        input.registry,
        input.source,
        &input.admission.github,
        input.git,
    )? != state
    {
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

fn metadata_action_matches(kind: JournalMutationKind, action: &CrateReleaseAction) -> bool {
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
    let state = observe_package_state(
        input.intent,
        input.request,
        input.github,
        input.registry,
        input.source,
        &input.admission.github,
        input.git,
    )?;
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
    if observe_package_state(
        input.intent,
        input.request,
        input.github,
        input.registry,
        input.source,
        &input.admission.github,
        input.git,
    )? != state
    {
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

pub(super) fn verify_binding<J: JournalIo, G: GitHubAdmissionRead, R: CrateRegistryRead>(
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

fn verify_bound_packages(
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

fn journal_authentication(
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

pub(super) fn verify_observed_packages_after_recorded_upload<
    J: JournalIo + crate::crate_release_journal::JournalOperationRefs,
    G,
    R,
>(
    input: &CrateReleaseObservation<'_, J, G, R>,
    current: &CrateReleaseAdmissionReceipt,
) -> Result<(), OrchestratorError> {
    verify_observed_packages_with_upload_intent(input, current, None)
}

pub(super) fn verify_observed_packages_with_upload_intent<
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

fn verify_observed_packages_with_history<
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

fn verify_creation_owner_transition(
    now: &super::CrateReleasePlannedPackage,
    old: &super::CrateReleasePlannedPackage,
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

fn derive_result<J: JournalIo, G: GitHubAdmissionRead, R: CrateRegistryRead>(
    input: &CrateReleaseObservation<'_, J, G, R>,
    package: &super::CrateReleasePlannedPackage,
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
            let archive = package_source(
                input.source,
                &effective,
                &package.source,
                target.path(),
                &input.request.source_sha,
                false,
                input.git,
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

fn invalid() -> OrchestratorError {
    internal("crate_release_verified_observation_invalid")
}

impl<J, G, R> std::fmt::Debug for CrateReleaseObservation<'_, J, G, R> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CrateReleaseObservation")
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum RecoveryState {
    Present,
    Missing,
    Blocked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum PriorRecordState {
    IntentionOnly,
    ObservationRecorded,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RecoveryOperationObservation {
    pub prior_identity: JournalIdentity,
    pub sequence: u32,
    pub package: String,
    pub version: String,
    pub kind: JournalMutationKind,
    pub prior_record: PriorRecordState,
    pub state: RecoveryState,
}

pub(crate) fn observe_prior_recovery<
    J: JournalIo + crate::crate_release_journal::JournalOperationRefs,
    G: GitHubAdmissionRead,
    R: CrateRegistryRead,
>(
    input: &CrateReleaseObservation<'_, J, G, R>,
    previous: &JournalIdentity,
) -> Result<Vec<RecoveryOperationObservation>, OrchestratorError> {
    verify_binding(input)?;
    authorize_github_candidate(input.request, &input.admission.github, input.github)?;
    let started = inspect_journal(input.journal, input.identity, JournalStage::Started)?
        .ok_or_else(invalid)?;
    if started.receipt != *input.journal_receipt {
        return Err(invalid());
    }
    if input.journal_receipt.predecessor.as_ref() != Some(previous) {
        return Err(invalid());
    }
    let history = crate::crate_release_journal::inspect_prior_chain(
        input.journal,
        input.journal_receipt,
        input.source,
        input.git,
    )?;
    let current = observe_crate_release(
        input.intent,
        input.request,
        input.github,
        input.registry,
        input.source,
        input.git,
    )?;
    verify_observed_packages(&current, input.admission)?;
    let mut results = Vec::new();
    for prior in &history {
        for (index, operation) in prior.mutations.iter().enumerate() {
            if !matches!(operation.phase, JournalMutationPhase::Intended) {
                continue;
            }
            let package = current
                .packages
                .iter()
                .find(|package| {
                    package.source.name == operation.package
                        && package.source.version == operation.version
                })
                .ok_or_else(invalid)?;
            let state = recovery_state(operation.kind, &package.action);
            results.push(RecoveryOperationObservation {
                prior_identity: prior.start.receipt.identity.clone(),
                sequence: operation.sequence,
                package: operation.package.clone(),
                version: operation.version.clone(),
                kind: operation.kind,
                prior_record: prior_record_state(prior.mutations.get(index + 1)),
                state,
            });
        }
    }
    if observe_crate_release(
        input.intent,
        input.request,
        input.github,
        input.registry,
        input.source,
        input.git,
    )? != current
        || crate::crate_release_journal::inspect_prior_chain(
            input.journal,
            input.journal_receipt,
            input.source,
            input.git,
        )? != history
    {
        return Err(invalid());
    }
    authorize_github_candidate(input.request, &input.admission.github, input.github)?;
    Ok(results)
}

fn prior_record_state(next: Option<&JournalMutation>) -> PriorRecordState {
    if next.is_some_and(|item| matches!(item.phase, JournalMutationPhase::Observed { .. })) {
        PriorRecordState::ObservationRecorded
    } else {
        PriorRecordState::IntentionOnly
    }
}

fn recovery_state(kind: JournalMutationKind, action: &CrateReleaseAction) -> RecoveryState {
    match (kind, action) {
        (JournalMutationKind::Upload, CrateReleaseAction::Publish)
        | (JournalMutationKind::CreateTag, CrateReleaseAction::RepairTag { .. })
        | (JournalMutationKind::CreateRelease, CrateReleaseAction::RepairRelease { .. }) => {
            RecoveryState::Missing
        }
        (JournalMutationKind::CreateTag, CrateReleaseAction::Publish)
        | (
            JournalMutationKind::CreateRelease,
            CrateReleaseAction::Publish | CrateReleaseAction::RepairTag { .. },
        ) => RecoveryState::Blocked,
        _ => RecoveryState::Present,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovery_state_respects_operation_prerequisites() {
        let registry = crate::crate_release_registry::CrateRegistryReconciliation {
            checksum: "d".repeat(64),
            downloaded_checksum: "d".repeat(64),
            owners: Vec::new(),
            features: std::collections::BTreeMap::new(),
        };
        let actions = [
            CrateReleaseAction::Publish,
            CrateReleaseAction::RepairTag {
                registry: registry.clone(),
            },
            CrateReleaseAction::RepairRelease {
                registry: registry.clone(),
            },
            CrateReleaseAction::Reconciled {
                registry,
                release: crate::crate_release_github::GitHubReleaseEvidence {
                    id: 1,
                    url: "https://github.com/example/demo/releases/tag/demo-v0.1.1".into(),
                },
            },
        ];
        let expected = [
            (
                JournalMutationKind::Upload,
                [
                    RecoveryState::Missing,
                    RecoveryState::Present,
                    RecoveryState::Present,
                    RecoveryState::Present,
                ],
            ),
            (
                JournalMutationKind::CreateTag,
                [
                    RecoveryState::Blocked,
                    RecoveryState::Missing,
                    RecoveryState::Present,
                    RecoveryState::Present,
                ],
            ),
            (
                JournalMutationKind::CreateRelease,
                [
                    RecoveryState::Blocked,
                    RecoveryState::Blocked,
                    RecoveryState::Missing,
                    RecoveryState::Present,
                ],
            ),
        ];
        for (kind, states) in expected {
            for (action, state) in actions.iter().zip(states) {
                assert_eq!(recovery_state(kind, action), state);
            }
        }
    }

    #[test]
    fn prior_record_state_distinguishes_recorded_observation_from_pending_intent() {
        let observed = JournalMutation {
            semver_evidence: None,
            sequence: 2,
            package: "demo".into(),
            version: "0.1.1".into(),
            kind: JournalMutationKind::CreateTag,
            phase: JournalMutationPhase::Observed {
                result: JournalMutationResult::Tag {
                    name: "demo-v0.1.1".into(),
                    target_sha: "a".repeat(40),
                },
            },
        };
        assert_eq!(prior_record_state(None), PriorRecordState::IntentionOnly);
        assert_eq!(
            prior_record_state(Some(&observed)),
            PriorRecordState::ObservationRecorded
        );
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReconciledCandidate {
    pub packages: Vec<super::CrateReleaseCompletedPackage>,
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

fn journal_owners(
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
    let packages = super::reconcile_crate_release(
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
