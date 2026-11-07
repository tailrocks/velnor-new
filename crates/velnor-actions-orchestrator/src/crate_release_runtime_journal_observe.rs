// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    CrateReleaseIntent, GitHubAdmissionRequest, JournalRuntime, NativeCrateRegistryRead,
    NativeGitHubAdmissionRead, OrchestratorError, Path, env, internal, read_receipt,
    reconcile_crate_release, required, required_path,
};

pub(super) fn observe_journal_operation(
    runtime: JournalRuntime<'_>,
    operation: &str,
) -> Result<(), OrchestratorError> {
    use crate::crate_release_controller::{
        CrateReleaseObservation, record_verified_mutation, repair_verified_metadata,
        verify_and_record_upload_intent,
    };
    use crate::crate_release_journal::{JournalIdentity, JournalStage, inspect_journal};
    use crate::crate_release_journal_adapter::JournalAdapter;
    let source_execution = runtime.upload.source_execution();
    let JournalRuntime {
        receipt_path,
        intent,
        request,
        github,
        registry,
        source,
        token,
        git,
        ..
    } = runtime;
    let value = required("VELNOR_JOURNAL_SEQUENCE")?;
    let sequence = value
        .parse::<u32>()
        .map_err(|_| internal("crate_release_sequence_invalid"))?;
    if !(1..=1000).contains(&sequence) || sequence.to_string() != value {
        return Err(internal("crate_release_sequence_invalid"));
    }
    let admission = read_receipt(receipt_path)?;
    let identity = JournalIdentity {
        repository: request.repository.clone(),
        workflow_sha: request.workflow_sha.clone(),
        source_sha: request.source_sha.clone(),
        run_id: request.run_id,
        run_attempt: request.run_attempt,
    };
    let journal = JournalAdapter::new(NativeGitHubAdmissionRead::new(token.to_owned())?);
    let started = inspect_journal(&journal, &identity, JournalStage::Started)?
        .ok_or_else(|| internal("crate_release_journal_start_missing"))?;
    let input = CrateReleaseObservation {
        source_execution,
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
        sequence,
    };
    match operation {
        "journal-upload-intent" => {
            let package = required("VELNOR_JOURNAL_PACKAGE")?;
            let semver = crate::release::crate_release_semver::verify_before_upload(
                source,
                &admission,
                registry,
                &required_path("RUNNER_TEMP")?,
                &package,
                git,
                source_execution,
            )?;
            verify_and_record_upload_intent(&input, &package, semver)?;
        }
        "journal-observe" => {
            record_verified_mutation(&input)?;
        }
        "journal-repair-tag" => repair_verified_metadata(
            &input,
            &required("VELNOR_JOURNAL_PACKAGE")?,
            crate::crate_release_journal::JournalMutationKind::CreateTag,
        )?,
        "journal-repair-release" => repair_verified_metadata(
            &input,
            &required("VELNOR_JOURNAL_PACKAGE")?,
            crate::crate_release_journal::JournalMutationKind::CreateRelease,
        )?,
        _ => return Err(internal("crate_release_operation_invalid")),
    }
    Ok(())
}

pub(super) fn verify_package(
    receipt_path: &Path,
    intent: &CrateReleaseIntent,
    request: &GitHubAdmissionRequest,
    github: &NativeGitHubAdmissionRead,
    source: &Path,
    git: &crate::crate_release_git::ReleaseGit,
    source_execution: &dyn crate::CrateSourceExecution,
) -> Result<(), OrchestratorError> {
    let receipt = read_receipt(receipt_path)?;
    let package = required("VELNOR_JOURNAL_PACKAGE")?;
    let input = crate::crate_release_controller::PackageVerification {
        source_execution,
        receipt: &receipt,
        intent,
        request,
        github,
        registry: &NativeCrateRegistryRead,
        source,
        git,
        package: &package,
    };
    crate::crate_release_controller::preflight::verify_release_package_with(&input, |observed| {
        crate::crate_release_controller::verify_observed_packages(observed, &receipt)
    })
    .map(|_| ())
}

pub(super) fn reconcile_receipt(
    receipt_path: &Path,
    intent: &CrateReleaseIntent,
    request: &GitHubAdmissionRequest,
    github: &NativeGitHubAdmissionRead,
    source: &Path,
    git: &crate::crate_release_git::ReleaseGit,
    source_execution: &dyn crate::CrateSourceExecution,
) -> Result<(), OrchestratorError> {
    let receipt = read_receipt(receipt_path)?;
    let completed = reconcile_crate_release(
        &receipt,
        intent,
        request,
        github,
        &NativeCrateRegistryRead,
        crate::CrateSource::new(source, git, source_execution),
    )?;
    let body = serde_json::to_vec(&serde_json::json!({"schema":1,"stage":"reconciled","receipt":receipt,"packages":completed,"generator_version":env!("CARGO_PKG_VERSION"),"workflow_run":format!("https://github.com/{}/actions/runs/{}", request.repository, request.run_id)})).map_err(|_| internal("crate_release_checkpoint_write"))?;
    velnor_actions_contract::exclusive_write::write_exclusive(
        &required_path("RUNNER_TEMP")?.join("velnor-crate-release-reconciled.json"),
        &body,
        "crate_release_checkpoint",
    )
    .map_err(Into::into)
}
