// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    CrateReleaseIntent, Duration, GitHubAdmissionRequest, GitRequest, Instant,
    NativeCrateRegistryRead, NativeGitHubAdmissionRead, OrchestratorError, Path, internal,
    parse_crate_release_intent, required, required_path,
};

pub(crate) fn read_intent_at_workflow(
    workspace: &Path,
    workflow_sha: &str,
    git: &crate::crate_release_git::ReleaseGit,
) -> Result<CrateReleaseIntent, OrchestratorError> {
    if !is_full_sha(workflow_sha) {
        return Err(internal("crate_release_workflow_sha"));
    }
    let show = GitRequest::show(vec![
        format!("{workflow_sha}:.github/velnor-release-intent.toml").into(),
    ])
    .run_release_in(workspace, git)
    .map_err(|_| internal("crate_release_intent_read"))?;
    if !show.success || show.stdout.len() > 1_048_576 {
        return Err(internal("crate_release_intent_read"));
    }
    parse_crate_release_intent(&show.stdout)
}

pub(super) fn read_manifest_at_workflow(
    workspace: &Path,
    workflow_sha: &str,
    git: &crate::crate_release_git::ReleaseGit,
) -> Result<Vec<u8>, OrchestratorError> {
    if !is_full_sha(workflow_sha) {
        return Err(internal("crate_release_workflow_sha"));
    }
    let show = GitRequest::show(vec![
        format!("{workflow_sha}:.velnor/release-manifest.json").into(),
    ])
    .run_release_in(workspace, git)
    .map_err(|_| internal("crate_release_reviewed_manifest_read"))?;
    if !show.success || show.stdout.is_empty() || show.stdout.len() > 1024 * 1024 {
        return Err(internal("crate_release_reviewed_manifest_read"));
    }
    Ok(show.stdout)
}

pub(super) fn candidate_receipt(
    intent: &CrateReleaseIntent,
    request: &GitHubAdmissionRequest,
    github: &NativeGitHubAdmissionRead,
    source: &Path,
    git: &crate::crate_release_git::ReleaseGit,
    source_execution: &dyn crate::CrateSourceExecution,
) -> Result<(), OrchestratorError> {
    let mut receipt = crate::crate_release_controller::prepare_crate_release(
        intent,
        request,
        github,
        &NativeCrateRegistryRead,
        crate::CrateSource::new(source, git, source_execution),
    )?;
    receipt.semver_evidence = crate::release::crate_release_semver::verify_preflight(
        source,
        &receipt,
        &NativeCrateRegistryRead,
        &required_path("RUNNER_TEMP")?,
        git,
        source_execution,
    )?;
    let data = serde_json::to_vec(&receipt).map_err(|_| internal("crate_release_receipt_write"))?;
    crate::crate_release_tool_artifact::write_receipt(&required_path("RUNNER_TEMP")?, &data)
}

pub(super) fn is_full_sha(sha: &str) -> bool {
    sha.len() == 40
        && sha
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

pub(super) fn acquire_toolchain(
    intent: &CrateReleaseIntent,
    request: &GitHubAdmissionRequest,
    github: &NativeGitHubAdmissionRead,
    token: &str,
) -> Result<(), OrchestratorError> {
    let artifact_id = required("VELNOR_RUST_ARTIFACT_ID")?
        .parse()
        .map_err(|_| internal("crate_release_tool_artifact_id"))?;
    let candidate = crate::crate_release_github::verify_github_evidence(request, github)?;
    crate::crate_release_tool_artifact::stage_unverified_toolchain(
        &crate::crate_release_tool_artifact::ToolchainStageRequest {
            intent,
            github: request,
            candidate: &candidate,
            artifact_id,
            token,
            runner_temp: &required_path("RUNNER_TEMP")?,
            deadline: Instant::now() + Duration::from_mins(15),
        },
        github,
        &crate::crate_release_tool_download::NativeToolDownloadTransport,
    )?;
    Ok(())
}

pub(super) fn acquire_receipt(
    intent: &CrateReleaseIntent,
    request: &GitHubAdmissionRequest,
    github: &NativeGitHubAdmissionRead,
    token: &str,
    source_context: crate::CrateSource<'_>,
    reconciliation: bool,
) -> Result<(), OrchestratorError> {
    let artifact_id = required("VELNOR_RECEIPT_ARTIFACT_ID")?
        .parse()
        .map_err(|_| internal("crate_release_receipt_artifact_id"))?;
    let candidate = crate::crate_release_github::verify_github_evidence(request, github)?;
    let acquire = if reconciliation {
        crate::crate_release_tool_artifact::acquire_reconciliation_receipt_artifact
    } else {
        crate::crate_release_tool_artifact::acquire_receipt_artifact
    };
    let stage = crate::crate_release_tool_artifact::ToolchainStageRequest {
        intent,
        github: request,
        candidate: &candidate,
        artifact_id,
        token,
        runner_temp: &required_path("RUNNER_TEMP")?,
        deadline: Instant::now() + Duration::from_mins(5),
    };
    if !reconciliation {
        return acquire(
            &stage,
            github,
            &NativeCrateRegistryRead,
            &crate::crate_release_tool_download::NativeToolDownloadTransport,
            source_context,
        );
    }
    crate::crate_release_controller::wait_for_reconciliation_visibility(
        12,
        || match acquire(
            &stage,
            github,
            &NativeCrateRegistryRead,
            &crate::crate_release_tool_download::NativeToolDownloadTransport,
            source_context,
        ) {
            Ok(()) => Ok(true),
            Err(OrchestratorError::Internal { problem })
                if problem == "crate_release_visibility_timeout" =>
            {
                Ok(false)
            }
            Err(error) => Err(error),
        },
        || std::thread::sleep(Duration::from_secs(5)),
    )
}
