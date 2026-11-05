// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

#[test]
fn admission_mac_and_finalizer_are_bound_in_generated_graph() -> Result<(), Box<dyn Error>> {
    let files = super::rendered()?;
    let text = super::find_file(&files, ".github/workflows/product-release-generator.yml")?;
    let workflow: serde_json::Value = serde_saphyr::from_str(text)?;
    let jobs = &workflow["jobs"];
    assert_eq!(jobs["qualify-homebrew-admit"]["runs-on"], "ubuntu-24.04");
    assert_eq!(jobs["qualify-homebrew-macos"]["runs-on"], "macos-15");
    assert_eq!(jobs["qualify-homebrew-finalize"]["runs-on"], "ubuntu-24.04");
    for (job, expected) in [
        ("qualify-homebrew-macos", "qualify-homebrew-admit"),
        ("qualify-homebrew-finalize", "qualify-homebrew-macos"),
        ("attest-macos", "qualify-homebrew-finalize"),
        ("publish-generator", "qualify-homebrew-finalize"),
    ] {
        let needs = jobs[job]["needs"].as_array().ok_or("needs")?;
        assert!(
            needs.iter().any(|value| value == expected),
            "{job} lacks {expected}"
        );
    }
    for (job, name) in [
        ("qualify-homebrew-admit", "admission_sha256"),
        ("qualify-homebrew-macos", "result_sha256"),
    ] {
        assert!(
            jobs[job]["outputs"]["artifact_id"]
                .as_str()
                .ok_or("artifact output")?
                .contains("steps.upload.outputs.artifact-id")
        );
        assert!(
            jobs[job]["outputs"][name]
                .as_str()
                .ok_or("digest output")?
                .contains("steps.evidence_digest.outputs")
        );
    }
    for job in ["qualify-homebrew-admit", "qualify-homebrew-macos"] {
        assert!(
            jobs[job]["outputs"]["artifact_digest"]
                .as_str()
                .ok_or("artifact digest")?
                .contains("steps.upload.outputs.artifact-digest")
        );
        assert!(
            jobs[job]["outputs"]["artifact_size"]
                .as_str()
                .ok_or("artifact size")?
                .contains("steps.evidence_digest.outputs.artifact_size")
        );
    }
    let final_steps = jobs["qualify-homebrew-finalize"]["steps"]
        .as_array()
        .ok_or("finalizer steps")?;
    for name in ["Download exact admission", "Download exact Mac result"] {
        let step = final_steps
            .iter()
            .find(|step| step["name"] == name)
            .ok_or("raw download")?;
        assert_eq!(step["with"]["skip-decompress"], true);
    }
    let action_text = super::find_file(
        &files,
        ".github/actions/generator-release-qualify-homebrew/action.yml",
    )?;
    let action: serde_json::Value = serde_saphyr::from_str(action_text)?;
    let raw = action["runs"]["steps"]
        .as_array()
        .ok_or("action steps")?
        .iter()
        .find(|step| step["name"] == "Download exact admission")
        .ok_or("raw admission")?;
    assert_eq!(raw["with"]["skip-decompress"], true);
    let publish_condition = jobs["publish-generator"]["if"]
        .as_str()
        .ok_or("publish condition")?;
    assert!(publish_condition.contains("needs.qualify-homebrew-finalize.result == 'success'"));
    Ok(())
}

#[test]
#[cfg(unix)]
fn generated_mac_command_does_not_inherit_tokens() -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::PermissionsExt as _;
    use std::process::Command;
    let files = super::rendered()?;
    let text = super::find_file(
        &files,
        ".github/actions/generator-release-qualify-homebrew/action.yml",
    )?;
    let action: serde_json::Value = serde_saphyr::from_str(text)?;
    let steps = action["runs"]["steps"].as_array().ok_or("action steps")?;
    let step = steps
        .iter()
        .find(|step| step["name"] == "Validate admission; install and test preserved Homebrew kegs")
        .ok_or("Mac step")?;
    assert!(step["env"].get("GH_TOKEN").is_none());
    for key in [
        "BASH_ENV",
        "ENV",
        "LD_PRELOAD",
        "LD_LIBRARY_PATH",
        "LD_AUDIT",
        "DYLD_INSERT_LIBRARIES",
        "DYLD_LIBRARY_PATH",
        "DYLD_FRAMEWORK_PATH",
    ] {
        assert_eq!(
            step["env"][key], "",
            "{key} must be cleared before the shell starts"
        );
    }
    let script = step["run"].as_str().ok_or("Mac run")?;
    let (prefix, _) = script
        .rsplit_once(" VELNOR_INTERNAL_OP=qualify-homebrew-v1 ")
        .ok_or("candidate command")?;
    let temp = tempfile::tempdir()?;
    let candidate = temp.path().join("candidate");
    std::fs::write(&candidate, b"#!/bin/sh\nset -eu\ntest -z \"${GH_TOKEN+x}${GITHUB_TOKEN+x}${ACTIONS_RUNTIME_TOKEN+x}${ACTIONS_ID_TOKEN_REQUEST_TOKEN+x}${ACTIONS_ID_TOKEN_REQUEST_URL+x}${AWS_SECRET_ACCESS_KEY+x}${GITHUB_APP_PRIVATE_KEY+x}\"\ntest \"$VELNOR_INTERNAL_OP\" = qualify-homebrew-v1\n")?;
    std::fs::set_permissions(&candidate, std::fs::Permissions::from_mode(0o700))?;
    let output = Command::new("/bin/bash")
        .arg("-c")
        .arg(format!(
            "{prefix} VELNOR_INTERNAL_OP=qualify-homebrew-v1 \"$FIXTURE_CANDIDATE\""
        ))
        .env("FIXTURE_CANDIDATE", &candidate)
        .env("GH_TOKEN", "dummy")
        .env("GITHUB_TOKEN", "dummy")
        .env("ACTIONS_RUNTIME_TOKEN", "dummy")
        .env("ACTIONS_ID_TOKEN_REQUEST_TOKEN", "dummy")
        .env("ACTIONS_ID_TOKEN_REQUEST_URL", "dummy")
        .env("AWS_SECRET_ACCESS_KEY", "dummy")
        .env("GITHUB_APP_PRIVATE_KEY", "dummy")
        .env("VELNOR_HOME_ADMISSION_SHA256", "a".repeat(64))
        .env("VELNOR_HOME_ADMISSION_ARCHIVE_SHA256", "d".repeat(64))
        .env("VELNOR_HOME_ADMISSION_ARCHIVE_SIZE", "42")
        .env("VELNOR_RELEASE_MANIFEST_SHA256", "b".repeat(64))
        .env("VELNOR_HOME_BUDGET_DEADLINE_EPOCH", "123")
        .env("RUNNER_TEMP", temp.path())
        .env("RUNNER_OS", "macOS")
        .env("RUNNER_ARCH", "ARM64")
        .env("RUNNER_ENVIRONMENT", "github-hosted")
        .env("GITHUB_ACTIONS", "true")
        .env("GITHUB_REPOSITORY", "owner/repo")
        .env("GITHUB_REPOSITORY_ID", "1")
        .env("GITHUB_RUN_ID", "2")
        .env("GITHUB_RUN_ATTEMPT", "1")
        .env(
            "GITHUB_WORKFLOW_REF",
            "owner/repo/.github/workflows/release.yml@refs/heads/main",
        )
        .env("GITHUB_WORKFLOW_SHA", "c".repeat(40))
        .env("GITHUB_SHA", "c".repeat(40))
        .env("GITHUB_WORKSPACE", temp.path())
        .output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}
