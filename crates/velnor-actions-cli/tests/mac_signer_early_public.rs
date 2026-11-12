// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    error::Error,
    fs,
    os::unix::fs::{MetadataExt, PermissionsExt},
    process::Command,
};

fn reject(job: Option<&str>, token: Option<&str>) -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let mise_bin = temp.path().join("bin");
    fs::create_dir(&mise_bin)?;
    let mise = mise_bin.join("mise");
    fs::write(
        &mise,
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$0.calls\"\nexit 99\n",
    )?;
    fs::set_permissions(&mise, fs::Permissions::from_mode(0o700))?;
    let calls_path = mise.with_extension("calls");
    let assets = temp.path().join("assets");
    fs::create_dir(&assets)?;
    let runner_temp = temp.path().join("runner-temp");
    fs::create_dir(&runner_temp)?;
    let output_path = temp.path().join("github-output");
    fs::write(&output_path, b"")?;
    let output_inode = fs::metadata(&output_path)?.ino();
    let mut command = Command::new(env!("CARGO_BIN_EXE_velnor-actions"));
    command
        .env_clear()
        .env("PATH", &mise_bin)
        .env("HOME", temp.path())
        .env("GITHUB_SHA", "a".repeat(40))
        .env("GITHUB_RUN_ID", "190001")
        .env("GITHUB_RUN_ATTEMPT", "1")
        .env("GITHUB_EVENT_NAME", "push")
        .env("GITHUB_REF", "refs/heads/main")
        .env("GITHUB_REF_PROTECTED", "true")
        .env("GITHUB_REPOSITORY", "tailrocks/velnor-new")
        .env(
            "GITHUB_WORKFLOW_REF",
            "tailrocks/velnor-new/.github/workflows/product-release-images.yml@refs/heads/main",
        )
        .env("GITHUB_WORKFLOW_SHA", "a".repeat(40))
        .env("GITHUB_OUTPUT", &output_path)
        .args([
            "image-maintenance",
            "verify-mac-attestation",
            "--producer-run-id",
            "190002",
            "--producer-run-attempt",
            "1",
            "--producer-artifact-id",
            "190003",
            "--publisher-artifact-id",
            "190004",
            "--assets-dir",
        ])
        .arg(&assets)
        .arg("--runner-temp")
        .arg(&runner_temp);
    if let Some(token) = token {
        command.env("GH_TOKEN", token);
    }
    if let Some(job) = job {
        command.env("GITHUB_JOB", job);
    }
    let result = command.output()?;
    assert!(!result.status.success());
    assert_eq!(fs::read(&output_path)?, b"");
    assert_eq!(fs::metadata(&output_path)?.ino(), output_inode);
    assert!(!calls_path.exists());
    Ok(())
}

#[test]
fn missing_github_token_fails_before_any_external_call_or_step_output() -> Result<(), Box<dyn Error>>
{
    reject(Some("verify-mac-proof"), None)
}

#[test]
fn missing_or_wrong_job_fails_before_any_external_call_or_step_output() -> Result<(), Box<dyn Error>>
{
    reject(None, Some("fixture-token"))?;
    reject(Some("publish-images"), Some("fixture-token"))
}
