// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

#[test]
fn admission_mac_and_finalizer_are_bound_in_generated_graph() -> Result<(), Box<dyn Error>> {
    let files = super::rendered()?;
    let text = super::find_file(&files, ".github/workflows/product-release-generator.yml")?;
    let workflow: serde_json::Value = serde_saphyr::from_str(text)?;
    let jobs = &workflow["jobs"];
    assert_eq!(jobs["qualify-homebrew-admit"]["runs-on"], "ubuntu-26.04");
    assert_eq!(jobs["qualify-homebrew-macos"]["runs-on"], "macos-15");
    assert_eq!(jobs["qualify-homebrew-finalize"]["runs-on"], "ubuntu-26.04");
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
    check_evidence_producers(jobs)?;
    let final_steps = jobs["qualify-homebrew-finalize"]["steps"]
        .as_array()
        .ok_or("finalizer steps")?;
    for name in ["Download exact admission", "Download exact Mac result"] {
        let index = final_steps
            .iter()
            .position(|step| step["name"] == name)
            .ok_or("raw download")?;
        assert_eq!(final_steps[index - 1]["name"], "Validate exact artifact ID");
        let step = &final_steps[index];
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

fn check_evidence_producers(jobs: &serde_json::Value) -> Result<(), Box<dyn Error>> {
    for (job, path) in [
        ("qualify-homebrew-admit", "homebrew-admission/admission.zip"),
        ("qualify-homebrew-macos", "homebrew-result/result.json"),
    ] {
        let steps = jobs[job]["steps"].as_array().ok_or("producer steps")?;
        let upload = steps
            .iter()
            .find(|step| step["id"] == "upload")
            .ok_or("upload")?;
        assert_eq!(upload["with"]["archive"], false);
        assert_eq!(upload["with"]["path"], path);
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

#[test]
#[cfg(unix)]
fn evidence_artifact_id_guard_rejects_empty_and_malformed_ids() -> Result<(), Box<dyn Error>> {
    let files = super::rendered()?;
    let text = super::find_file(
        &files,
        ".github/actions/generator-release-qualify-homebrew/action.yml",
    )?;
    let action: serde_json::Value = serde_saphyr::from_str(text)?;
    let steps = action["runs"]["steps"].as_array().ok_or("steps")?;
    let index = steps
        .iter()
        .position(|step| step["name"] == "Download exact admission")
        .ok_or("download")?;
    let guard = &steps[index - 1];
    assert_eq!(guard["name"], "Validate exact artifact ID");
    let script = guard["run"].as_str().ok_or("guard")?;
    for (id, expected) in [
        ("123", true),
        ("", false),
        ("0", false),
        ("01", false),
        ("1 2", false),
        ("1;exit 0", false),
    ] {
        let status = std::process::Command::new("/bin/bash")
            .arg("-c")
            .arg(script)
            .env("VELNOR_EVIDENCE_ARTIFACT_ID", id)
            .status()?;
        assert_eq!(status.success(), expected, "{id:?}");
    }
    Ok(())
}

#[test]
fn every_generated_artifact_id_download_is_preceded_by_its_guard() -> Result<(), Box<dyn Error>> {
    for file in super::rendered()? {
        if std::path::Path::new(&file.path)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("yml"))
        {
            let yaml: serde_json::Value = serde_saphyr::from_str(&file.bytes)?;
            check_download_guards(&yaml)?;
        }
    }
    Ok(())
}

fn check_download_guards(value: &serde_json::Value) -> Result<(), Box<dyn Error>> {
    match value {
        serde_json::Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                if let Some(id) = item
                    .pointer("/with/artifact-ids")
                    .and_then(serde_json::Value::as_str)
                {
                    let previous = index
                        .checked_sub(1)
                        .and_then(|i| items.get(i))
                        .ok_or("missing artifact guard")?;
                    assert_eq!(previous["name"], "Validate exact artifact ID");
                    assert_eq!(previous["env"]["VELNOR_EVIDENCE_ARTIFACT_ID"], id);
                    assert_eq!(previous.get("if"), item.get("if"));
                }
                check_download_guards(item)?;
            }
        }
        serde_json::Value::Object(fields) => {
            for child in fields.values() {
                check_download_guards(child)?;
            }
        }
        _ => {}
    }
    Ok(())
}
