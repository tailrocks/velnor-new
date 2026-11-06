// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{Github, fs, velnor_actions_orchestrator};
use velnor_actions_orchestrator::crate_release_tool_artifact::{
    ToolchainStageRequest, stage_unverified_toolchain,
};

pub(super) fn check_post_extraction(
    stage: &ToolchainStageRequest<'_>,
    github: &Github,
    parent: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let revoked = parent.join("revoked");
    fs::create_dir_all(revoked.join("velnor"))?;
    github.approved.set(true);
    github.approval_reads.set(0);
    github.revoke_at_approval.set(3);
    let request = ToolchainStageRequest {
        runner_temp: &revoked,
        ..*stage
    };
    assert!(stage_unverified_toolchain(&request, github, github).is_err());
    assert!(revoked.join("velnor/rustup").is_dir());
    github.revoke_at_approval.set(0);
    github.approved.set(true);

    let altered = parent.join("altered");
    fs::create_dir_all(altered.join("velnor"))?;
    github.artifact_reads.set(0);
    github.alter_at_artifact.set(5);
    let request = ToolchainStageRequest {
        runner_temp: &altered,
        ..*stage
    };
    assert!(stage_unverified_toolchain(&request, github, github).is_err());
    assert!(altered.join("velnor/rustup").is_dir());
    github.alter_at_artifact.set(0);

    let weakened = parent.join("weakened");
    fs::create_dir_all(weakened.join("velnor"))?;
    github.ruleset_reads.set(0);
    github.revoke_at_ruleset.set(3);
    let request = ToolchainStageRequest {
        runner_temp: &weakened,
        ..*stage
    };
    assert!(stage_unverified_toolchain(&request, github, github).is_err());
    assert!(weakened.join("velnor/rustup").is_dir());
    github.revoke_at_ruleset.set(0);
    Ok(())
}

pub(super) fn check_paginated(
    stage: &ToolchainStageRequest<'_>,
    github: &Github,
    parent: &std::path::Path,
    host: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let runner = parent.canonicalize()?.join("paginated-runner");
    fs::create_dir_all(runner.join("velnor"))?;
    let request = ToolchainStageRequest {
        runner_temp: &runner,
        ..*stage
    };
    github.paginated_artifacts.set(true);
    let result = stage_unverified_toolchain(&request, github, github)?;
    assert_eq!(result.artifact().id, 10);
    assert_eq!(
        fs::read(result.path().join(format!(
            "toolchains/{}-{host}/bin/cargo",
            stage.intent.rust_version
        )))?,
        b"x"
    );
    github.paginated_artifacts.set(false);
    Ok(())
}
