// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{TestResult, sha256};
use serde_json::json;
use std::{
    fs,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

fn assert_refusal(expected: &str) -> TestResult {
    let error = crate::execute_homebrew_qualification_from_env()
        .err()
        .ok_or("unadmitted qualification succeeded")?;
    assert!(
        error.to_string().contains(expected),
        "unexpected refusal: {error}"
    );
    assert!(!std::path::Path::new("homebrew-result").exists());
    Ok(())
}

#[test]
fn public_homebrew_execution_refuses_unadmitted_bytes_and_credentials_before_host_mutation()
-> TestResult {
    if let Ok(expected) = std::env::var("VELNOR_INTERNAL_OP") {
        return assert_refusal(&expected);
    }
    let root = tempfile::tempdir()?;
    fs::create_dir(root.path().join("manifest-assets"))?;
    fs::create_dir(root.path().join("homebrew-admission-zip"))?;
    let raw = super::zip_bundle::fixture_bundle(&[("admission.json", b"unadmitted")])?;
    fs::write(
        root.path()
            .join("homebrew-admission-zip/downloaded-artifact"),
        &raw,
    )?;
    let executable = std::env::current_exe()?;
    let executable_digest = sha256(&fs::read(&executable)?);
    let source = "a".repeat(40);
    let targets = ["aarch64-apple-darwin", "x86_64-apple-darwin", "x86_64-unknown-linux-gnu"].into_iter().map(|target| json!({"target": target, "artifact": format!("https://github.com/tailrocks/velnor-new/releases/download/generator-{source}/velnor-actions-0.1.74-{target}"), "sha256": executable_digest})).collect::<Vec<_>>();
    let manifest = velnor_actions_contract::canonical_json_bytes(
        &json!({"schema": 1, "version": "0.1.74", "repository": "tailrocks/velnor-new", "commit": source, "targets": targets}),
    )?;
    fs::write(
        root.path().join("manifest-assets/release-manifest.json"),
        &manifest,
    )?;
    let deadline = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() + 300;
    let test_name = "homebrew::qualification::handoff::tests::public_entry::public_homebrew_execution_refuses_unadmitted_bytes_and_credentials_before_host_mutation";
    for credential in [
        None,
        Some("GH_TOKEN"),
        Some("ACTIONS_RUNTIME_TOKEN"),
        Some("ACTIONS_ID_TOKEN_REQUEST_TOKEN"),
    ] {
        let mut command = Command::new(&executable);
        let expected = if credential.is_some() {
            "homebrew_handoff_credentials_present"
        } else {
            "homebrew_handoff_admission_digest"
        };
        command
            .env_clear()
            .current_dir(root.path())
            .args(["--exact", test_name, "--nocapture"])
            .env("VELNOR_INTERNAL_OP", expected)
            .env("VELNOR_HOME_BUDGET_DEADLINE_EPOCH", deadline.to_string())
            .env("VELNOR_RELEASE_MANIFEST_SHA256", sha256(&manifest))
            .env("VELNOR_HOME_ADMISSION_SHA256", "0".repeat(64))
            .env("VELNOR_HOME_ADMISSION_ARCHIVE_SHA256", sha256(&raw))
            .env("VELNOR_HOME_ADMISSION_ARCHIVE_SIZE", raw.len().to_string())
            .env("GITHUB_REPOSITORY", "tailrocks/velnor-new")
            .env("GITHUB_REPOSITORY_ID", "105")
            .env("GITHUB_SHA", &source)
            .env("GITHUB_RUN_ID", "204")
            .env("GITHUB_RUN_ATTEMPT", "2")
            .env(
                "GITHUB_WORKFLOW_REF",
                "tailrocks/velnor-new/.github/workflows/generator-release.yml@refs/heads/main",
            )
            .env("GITHUB_WORKFLOW_SHA", "b".repeat(40));
        if let Some(key) = credential {
            command.env(key, "must-not-reach-homebrew");
        }
        let output = command.output()?;
        assert!(
            output.status.success(),
            "public boundary failed: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("1 passed"),
            "subprocess did not select public boundary test"
        );
        assert!(!root.path().join("homebrew-result").exists());
    }
    Ok(())
}
