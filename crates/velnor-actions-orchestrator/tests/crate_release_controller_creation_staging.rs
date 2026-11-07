// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{GitHubAdmissionRequest, Github, fs, velnor_actions_orchestrator};

#[cfg(unix)]
pub(super) fn check_creation_staging(
    intent: &velnor_actions_contract::config::CrateReleaseIntent,
    request: &GitHubAdmissionRequest,
    github: &Github,
    stage: &velnor_actions_orchestrator::crate_release_tool_artifact::ToolchainStageRequest<'_>,
    result: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    use velnor_actions_orchestrator::crate_release_tool_artifact::{
        ToolchainStageRequest, stage_unverified_toolchain,
    };
    fs::remove_dir_all(result)?;
    let mut creation = intent.clone();
    creation.recovery = None;
    creation.authentication =
        velnor_actions_contract::config::ReleaseAuthentication::BootstrapToken;
    creation.new_crates = Some(velnor_actions_contract::config::NewCrateRelease {
        source_sha: request.source_sha.clone(),
        packages: [("demo".into(), "0.1.1".into())].into(),
    });
    let mut push = request.clone();
    push.event = "push".into();
    push.source_sha = push.workflow_sha.clone();
    push.recovery_sha = None;
    let rejected = ToolchainStageRequest {
        intent: &creation,
        github: &push,
        ..*stage
    };
    let Err(error) = stage_unverified_toolchain(&rejected, github, github) else {
        return Err("new-crate push was accepted".into());
    };
    assert!(
        error
            .to_string()
            .contains("crate_release_toolchain_staging_invalid")
    );
    let creation_stage = ToolchainStageRequest {
        intent: &creation,
        ..*stage
    };
    assert_eq!(
        stage_unverified_toolchain(&creation_stage, github, github)?.path(),
        result
    );
    Ok(())
}
