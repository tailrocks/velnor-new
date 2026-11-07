// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn continuation_accepts_reviewed_tool_changes_but_not_release_content()
-> Result<(), Box<dyn std::error::Error>> {
    let current: CrateReleaseIntent = serde_json::from_value(serde_json::json!({
        "schema":1,"repository":"owner/project","default_branch":"main","environment":"release",
        "required_workflow_path":".github/workflows/ci.yml","trusted_reviewer_id":139_017,
        "generator_version":"0.1.72","generator_source_sha":"c".repeat(40),"generator_manifest_sha256":"d".repeat(64),
        "release_plz_version":"0.4.3","runner":"ubuntu-26.04","authentication":"trusted-publishing",
        "rust_version":"1.97.1","manifest_path":"Cargo.toml","pr_write_paths":["Cargo.toml"],
        "registry":"crates-io","packages":["demo"],"publish_features":{"demo":[]},
        "tag_template":"{{ package }}-v{{ version }}"
    }))?;
    let expected = crate::crate_release_journal::release_base_intent_sha256(&current)?;
    let mut prior = current.clone();
    prior.generator_version = "0.1.71".into();
    prior.generator_source_sha = "e".repeat(40);
    prior.generator_manifest_sha256 = "f".repeat(64);
    assert_eq!(normalized_historical_base(&prior, &current)?, expected);
    prior
        .publish_features
        .insert("demo".into(), vec!["changed".into()]);
    assert_ne!(normalized_historical_base(&prior, &current)?, expected);
    Ok(())
}

#[test]
fn direct_reader_hashes_real_git_objects_and_rejects_incomplete_history()
-> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    std::fs::create_dir(root.join(".github"))?;
    let intent: CrateReleaseIntent = serde_json::from_value(serde_json::json!({
        "schema":1,"repository":"owner/project","default_branch":"main","environment":"release",
        "required_workflow_path":".github/workflows/ci.yml","trusted_reviewer_id":139_017,
        "generator_version":"0.1.72","generator_source_sha":"c".repeat(40),"generator_manifest_sha256":"d".repeat(64),
        "release_plz_version":"0.4.3","runner":"ubuntu-26.04","authentication":"trusted-publishing",
        "rust_version":"1.97.1","manifest_path":"Cargo.toml","pr_write_paths":["Cargo.toml"],
        "registry":"crates-io","packages":["demo"],"publish_features":{"demo":[]},
        "tag_template":"{{ package }}-v{{ version }}"
    }))?;
    intent.validate()?;
    std::fs::write(
        root.join(".github/velnor-release-intent.toml"),
        toml::to_string(&intent)?,
    )?;
    for args in [
        &["init", "-q"][..],
        &["add", "."][..],
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "-qm",
            "policy",
        ][..],
    ] {
        assert!(
            std::process::Command::new("git")
                .current_dir(root)
                .args(args)
                .status()?
                .success()
        );
    }
    let output = std::process::Command::new("git")
        .current_dir(root)
        .args(["rev-parse", "HEAD"])
        .output()?;
    assert!(output.status.success());
    let sha = String::from_utf8(output.stdout)?.trim().to_owned();
    let git = crate::crate_release_git::ReleaseGit::host_fixture();
    let deadline = CheckDeadline::after(std::time::Duration::from_secs(30))?;
    assert_eq!(
        read_historical_intent(root, &sha, &git, deadline, &DirectHistoryWatch)?,
        intent
    );
    std::fs::write(root.join(".git/shallow"), format!("{sha}\n"))?;
    assert!(read_historical_intent(root, &sha, &git, deadline, &DirectHistoryWatch).is_err());
    Ok(())
}
