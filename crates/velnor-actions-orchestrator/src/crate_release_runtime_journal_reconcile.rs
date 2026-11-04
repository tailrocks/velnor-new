// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    CrateReleaseIntent, GitHubAdmissionRequest, JournalRuntime, NativeCrateRegistryRead,
    NativeGitHubAdmissionRead, OrchestratorError, Path, internal, read_receipt, release_mode,
    required, required_path, verify_crate_release_admission, write_checkpoint,
};

pub(super) fn reconcile_journal_evidence(
    input: JournalRuntime<'_>,
) -> Result<(), OrchestratorError> {
    use crate::crate_release_controller::{CrateReleaseObservation, derive_reconciled_candidate};
    use crate::crate_release_journal::{JournalIdentity, JournalStage, inspect_journal};
    use crate::crate_release_journal_adapter::JournalAdapter;
    let JournalRuntime {
        receipt_path,
        intent,
        request,
        github,
        registry,
        source,
        token,
        git,
    } = input;
    let identity = JournalIdentity {
        repository: request.repository.clone(),
        workflow_sha: request.workflow_sha.clone(),
        source_sha: request.source_sha.clone(),
        run_id: request.run_id,
        run_attempt: request.run_attempt,
    };
    let admission = read_receipt(receipt_path)?;
    let journal = JournalAdapter::new(NativeGitHubAdmissionRead::new(token.to_owned())?);
    let started = inspect_journal(&journal, &identity, JournalStage::Started)?
        .ok_or_else(|| internal("crate_release_journal_start_missing"))?;
    let input = CrateReleaseObservation {
        journal: &journal,
        identity: &identity,
        journal_receipt: &started.receipt,
        admission: &admission,
        intent,
        request,
        github,
        registry,
        source,
        git,
        sequence: 0,
    };
    let candidate = derive_reconciled_candidate(&input)?;
    let data =
        serde_json::to_vec(&candidate).map_err(|_| internal("crate_release_receipt_write"))?;
    let path = required_path("RUNNER_TEMP")?.join("velnor-reconciled-candidate.json");
    crate::exclusive_write::write_exclusive(&path, &data, "crate_release_candidate")
}

pub(super) fn inspect_prior_journal(input: JournalRuntime<'_>) -> Result<(), OrchestratorError> {
    use crate::crate_release_controller::{CrateReleaseObservation, observe_prior_recovery};
    use crate::crate_release_journal::{JournalIdentity, JournalStage, inspect_journal};
    use crate::crate_release_journal_adapter::JournalAdapter;
    let JournalRuntime {
        receipt_path,
        intent,
        request,
        github,
        registry,
        source,
        token,
        git,
    } = input;
    let run_text = required("VELNOR_PREVIOUS_RUN_ID")?;
    let run_id = run_text
        .parse::<u64>()
        .map_err(|_| internal("crate_release_previous_invalid"))?;
    let attempt_text = required("VELNOR_PREVIOUS_RUN_ATTEMPT")?;
    let run_attempt = attempt_text
        .parse::<u32>()
        .map_err(|_| internal("crate_release_previous_invalid"))?;
    if run_id == 0
        || run_id.to_string() != run_text
        || run_attempt == 0
        || run_attempt.to_string() != attempt_text
    {
        return Err(internal("crate_release_previous_invalid"));
    }
    let previous = JournalIdentity {
        repository: request.repository.clone(),
        workflow_sha: required("VELNOR_PREVIOUS_WORKFLOW_SHA")?,
        source_sha: request.source_sha.clone(),
        run_id,
        run_attempt,
    };
    let identity = JournalIdentity {
        repository: request.repository.clone(),
        workflow_sha: request.workflow_sha.clone(),
        source_sha: request.source_sha.clone(),
        run_id: request.run_id,
        run_attempt: request.run_attempt,
    };
    let admission = read_receipt(receipt_path)?;
    let journal = JournalAdapter::new(NativeGitHubAdmissionRead::new(token.to_owned())?);
    let started = inspect_journal(&journal, &identity, JournalStage::Started)?
        .ok_or_else(|| internal("crate_release_journal_start_missing"))?;
    let input = CrateReleaseObservation {
        journal: &journal,
        identity: &identity,
        journal_receipt: &started.receipt,
        admission: &admission,
        intent,
        request,
        github,
        registry,
        source,
        git,
        sequence: 0,
    };
    let evidence = observe_prior_recovery(&input, &previous)?;
    let data =
        serde_json::to_vec(&evidence).map_err(|_| internal("crate_release_receipt_write"))?;
    let path = required_path("RUNNER_TEMP")?.join("velnor-prior-recovery-evidence.json");
    crate::exclusive_write::write_exclusive(&path, &data, "crate_release_prior_evidence")
}

pub(super) fn checkpoint_publication(
    receipt_path: &Path,
    intent: &CrateReleaseIntent,
    request: &GitHubAdmissionRequest,
    github: &NativeGitHubAdmissionRead,
    registry: &NativeCrateRegistryRead,
    source: &Path,
    git: &crate::crate_release_git::ReleaseGit,
) -> Result<(), OrchestratorError> {
    let receipt = read_receipt(receipt_path)?;
    verify_crate_release_admission(&receipt, intent, request, github, registry, source, git)?;
    if release_mode(&receipt) == "none" {
        return Err(internal("crate_release_no_mutation"));
    }
    write_checkpoint(
        &required_path("RUNNER_TEMP")?.join("velnor-crate-release-publication-started.json"),
        "publication-started",
        &receipt,
    )
}
