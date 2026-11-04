// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    Duration, JournalRuntime, NativeGitHubAdmissionRead, OrchestratorError, internal,
    read_manifest_at_workflow, read_receipt, required, required_path, start_journal_with, thread,
    verify_bootstrap_attestation,
};

pub(super) fn coordinate_journal(runtime: JournalRuntime<'_>) -> Result<(), OrchestratorError> {
    use crate::crate_release_controller::{
        CrateReleaseObservation, coordinate_release, invoke_upload,
    };
    use crate::crate_release_journal::{JournalIdentity, JournalStage, inspect_journal};
    use crate::crate_release_journal_adapter::JournalAdapter;
    use velnor_actions_mise::command::CratePublishCredential;

    let identity = JournalIdentity {
        repository: runtime.request.repository.clone(),
        workflow_sha: runtime.request.workflow_sha.clone(),
        source_sha: runtime.request.source_sha.clone(),
        run_id: runtime.request.run_id,
        run_attempt: runtime.request.run_attempt,
    };
    let admission = read_receipt(runtime.receipt_path)?;
    let journal = JournalAdapter::new(NativeGitHubAdmissionRead::new(runtime.token.to_owned())?);
    let started = inspect_journal(&journal, &identity, JournalStage::Started)?
        .ok_or_else(|| internal("crate_release_journal_start_missing"))?;
    let input = CrateReleaseObservation {
        journal: &journal,
        identity: &identity,
        journal_receipt: &started.receipt,
        admission: &admission,
        intent: runtime.intent,
        request: runtime.request,
        github: runtime.github,
        registry: runtime.registry,
        source: runtime.source,
        git: runtime.git,
        sequence: 1,
    };
    let runner_temp = required_path("RUNNER_TEMP")?;
    coordinate_release(
        &input,
        |package| {
            crate::crate_release_semver::verify_before_upload(
                runtime.source,
                &admission,
                runtime.registry,
                &runner_temp,
                package,
                runtime.git,
            )
        },
        |permit| {
            let (token, url) = upload_credentials(runtime.intent.authentication)?;
            let credential = match &url {
                Some(request_url) => CratePublishCredential::Trusted {
                    request_url,
                    request_token: &token,
                },
                None => CratePublishCredential::Bootstrap {
                    cargo_token: &token,
                },
            };
            invoke_upload(&input, permit, &runner_temp, runtime.token, credential)
        },
        || thread::sleep(coordinator_wait_duration()),
    )?;
    let reviewed_manifest = read_manifest_at_workflow(
        &required_path("GITHUB_WORKSPACE")?,
        &runtime.request.workflow_sha,
        runtime.git,
    )?;
    start_journal_with(
        &runner_temp.join("velnor/bootstrap"),
        runtime.intent,
        &reviewed_manifest,
        verify_bootstrap_attestation,
        |generator| {
            let tools = crate::package_release_signer::measure_release_tools(
                &runner_temp,
                runtime.intent,
                &started.receipt,
                &generator,
            )?;
            crate::crate_release_journal::record_reconciled_completion(&input, tools)?;
            Ok(())
        },
    )
}

pub(super) fn coordinator_wait_duration() -> Duration {
    #[cfg(test)]
    if std::env::var_os("VELNOR_TEST_FAST_POLL").is_some() {
        return Duration::ZERO;
    }
    Duration::from_secs(2)
}

pub(super) fn upload_credentials(
    authentication: velnor_actions_contract::config::ReleaseAuthentication,
) -> Result<(String, Option<String>), OrchestratorError> {
    use velnor_actions_contract::config::ReleaseAuthentication;
    match authentication {
        ReleaseAuthentication::TrustedPublishing => Ok((
            required("ACTIONS_ID_TOKEN_REQUEST_TOKEN")?,
            Some(required("ACTIONS_ID_TOKEN_REQUEST_URL")?),
        )),
        ReleaseAuthentication::BootstrapToken => Ok((required("CARGO_REGISTRY_TOKEN")?, None)),
    }
}
