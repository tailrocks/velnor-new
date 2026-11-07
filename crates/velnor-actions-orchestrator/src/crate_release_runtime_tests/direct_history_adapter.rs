// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::crate_release_github::GitHubAdmissionRequest;
use crate::crate_release_journal::{JournalIo as _, JournalReceipt, JournalTag};
use std::path::Path;

pub(super) fn exercise(
    root: &Path,
    policy: &str,
    source: &str,
    request: &GitHubAdmissionRequest,
    receipt: &JournalReceipt,
    prior: &JournalReceipt,
) -> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::symlink;
    let source_dir = root.join("release-source");
    assert!(
        std::process::Command::new("git")
            .args(["clone", "--quiet", "--no-hardlinks", "--local"])
            .arg(root)
            .arg(&source_dir)
            .status()?
            .success()
    );
    assert!(
        std::process::Command::new("git")
            .current_dir(&source_dir)
            .args(["checkout", "--quiet", "--detach", source])
            .status()?
            .success()
    );
    let runner = tempfile::tempdir()?;
    let bundle = runner.path().join("velnor/git-bundle");
    std::fs::create_dir_all(runner.path().join("velnor"))?;
    match (
        std::env::var_os("VELNOR_TEST_PINNED_GIT"),
        std::env::var_os("VELNOR_TEST_PINNED_GIT_HTTP"),
    ) {
        (Some(git), Some(http)) => {
            std::fs::create_dir(&bundle)?;
            std::fs::copy(git, bundle.join("git"))?;
            std::fs::copy(http, bundle.join("git-remote-http"))?;
            symlink("git-remote-http", bundle.join("git-remote-https"))?;
            velnor_actions_mise::release_git_pin::verify_bundle(&bundle)?;
        }
        (None, None) => {
            let deadline =
                velnor_actions_mise::CheckDeadline::after(std::time::Duration::from_secs(120))?;
            crate::release::crate_release_git_bundle::acquire(runner.path(), deadline)?;
        }
        _ => return Err("incomplete pinned Git fixture".into()),
    }
    let output = std::process::Command::new(std::env::current_exe()?)
        .args([
            "--exact",
            "release::crate_release_runtime::tests::test_only_dispatch_and_loopback_transport",
        ])
        .env("VELNOR_TEST_DIRECT_HISTORY_CHILD", "1")
        .env(
            "VELNOR_TEST_DIRECT_REQUEST",
            serde_json::to_string(request)?,
        )
        .env(
            "VELNOR_TEST_DIRECT_RECEIPT",
            serde_json::to_string(receipt)?,
        )
        .env("VELNOR_TEST_DIRECT_PRIOR", serde_json::to_string(prior)?)
        .env("GITHUB_WORKSPACE", root)
        .env("RUNNER_TEMP", runner.path())
        .env("GITHUB_REPOSITORY", "owner/project")
        .env("GITHUB_EVENT_NAME", "workflow_dispatch")
        .env("GITHUB_REF", "refs/heads/main")
        .env(
            "GITHUB_WORKFLOW_REF",
            "owner/project/.github/workflows/release.yml@refs/heads/main",
        )
        .env("GITHUB_WORKFLOW_SHA", policy)
        .env("GITHUB_RUN_ID", "12")
        .env("GITHUB_RUN_ATTEMPT", "1")
        .output()?;
    assert!(
        output.status.success(),
        "direct adapter failed: {} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    crate::libtest_case::assert_one_passed(&output);
    Ok(())
}

pub(in super::super) fn verify_direct_history_child() -> Result<(), Box<dyn std::error::Error>> {
    use crate::crate_release_github::GitHubAdmissionRead as _;
    if std::env::var_os("VELNOR_TEST_SAME_BASE_SIGNED").is_some() {
        verify_signed_generator_fixture()?;
    }
    let request: GitHubAdmissionRequest =
        serde_json::from_str(&std::env::var("VELNOR_TEST_DIRECT_REQUEST")?)?;
    let receipt: JournalReceipt =
        serde_json::from_str(&std::env::var("VELNOR_TEST_DIRECT_RECEIPT")?)?;
    let github = crate::crate_release_github::NativeGitHubAdmissionRead::new("fixture".into())?;
    let previous =
        crate::release::crate_release_direct_history::verify_prior(&receipt, &request, &github)?;
    assert_eq!(
        previous.exact_source_sha(),
        Some(request.source_sha.as_str())
    );
    let prior: JournalReceipt = serde_json::from_str(&std::env::var("VELNOR_TEST_DIRECT_PRIOR")?)?;
    github.get(&format!(
        "repos/owner/project/compare/{}...{}",
        request.source_sha, prior.identity.workflow_sha
    ))?;
    crate::release::crate_release_direct_history::verify_transition(
        &prior, &receipt, &request, &github,
    )?;
    exercise_interrupted_journal(&prior, &receipt, &request, &github)?;
    if std::env::var_os("VELNOR_TEST_CONTROLLER_SCENARIO").is_some()
        && std::env::var_os("VELNOR_TEST_SAME_BASE_SIGNED").is_some()
        && std::env::var_os("VELNOR_TEST_FORGED_TOOL").is_none()
    {
        crate::crate_release_controller::tests::signed_history::exercise()?;
    }
    verify_forged_transitions(&prior, &receipt, &request, &github);
    let mut invalid = receipt.clone();
    invalid.intent_sha256 = "0".repeat(64);
    assert!(
        crate::release::crate_release_direct_history::verify_prior(&invalid, &request, &github)
            .is_err()
    );
    let manifest = std::path::PathBuf::from(std::env::var("GITHUB_WORKSPACE")?)
        .join("release-source/Cargo.toml");
    std::fs::write(manifest, "[package]\nname='tampered'\nversion='0.1.1'\n")?;
    assert!(
        crate::release::crate_release_direct_history::verify_prior(&receipt, &request, &github)
            .is_err(),
        "changed source must invalidate history"
    );
    Ok(())
}

fn verify_forged_transitions(
    prior: &JournalReceipt,
    receipt: &JournalReceipt,
    request: &GitHubAdmissionRequest,
    github: &crate::crate_release_github::NativeGitHubAdmissionRead,
) {
    let mut forged_prior = prior.clone();
    forged_prior.intent_sha256 = "0".repeat(64);
    assert!(
        crate::release::crate_release_direct_history::verify_transition(
            &forged_prior,
            receipt,
            request,
            github
        )
        .is_err()
    );
    let mut wrong_origin = prior.clone();
    wrong_origin.identity.run_id = 13;
    wrong_origin.workflow_run_url = "https://github.com/owner/project/actions/runs/13".into();
    assert!(
        crate::release::crate_release_direct_history::verify_transition(
            &wrong_origin,
            receipt,
            request,
            github
        )
        .is_err(),
        "a different run origin must fail"
    );
    let mut wrong_runner = prior.clone();
    wrong_runner
        .identity
        .workflow_sha
        .clone_from(&request.workflow_sha);
    assert!(
        crate::release::crate_release_direct_history::verify_transition(
            &wrong_runner,
            receipt,
            request,
            github
        )
        .is_err(),
        "the historical runner transition must match"
    );
}

fn verify_signed_generator_fixture() -> Result<(), Box<dyn std::error::Error>> {
    let source =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/attestation_fixtures");
    let temp = tempfile::tempdir()?;
    let binary = "velnor-actions-0.1.5-x86_64-unknown-linux-gnu";
    for name in [
        "release-manifest.json",
        "release-manifest.json.intoto.jsonl",
        "velnor-actions-0.1.5-x86_64-unknown-linux-gnu.intoto.jsonl",
    ] {
        std::fs::copy(source.join(name), temp.path().join(name))?;
    }
    let compressed = std::fs::File::open(source.join("binary.gz"))?;
    let mut decoder = flate2::read::GzDecoder::new(compressed);
    let mut output = std::fs::File::create(temp.path().join(binary))?;
    std::io::copy(&mut decoder, &mut output)?;
    let deadline = velnor_actions_mise::CheckDeadline::after(std::time::Duration::from_secs(180))?;
    let (_, _, _generator) =
        crate::release::package_release_signer::verified_generator_fixture(temp.path(), deadline)?;
    Ok(())
}

fn exercise_interrupted_journal(
    prior: &JournalReceipt,
    current: &JournalReceipt,
    request: &GitHubAdmissionRequest,
    github: &crate::crate_release_github::NativeGitHubAdmissionRead,
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::crate_release_journal::{
        JournalIntendedMutation, JournalMutationKind, fixture_storage::seed_started_record,
    };
    let root = std::path::PathBuf::from(std::env::var("GITHUB_WORKSPACE")?).join("release-source");
    let bundle = std::path::PathBuf::from(std::env::var("RUNNER_TEMP")?).join("velnor/git-bundle");
    let git = crate::crate_release_git::ReleaseGit::new(
        velnor_actions_mise::release_git_pin::verify_bundle(&bundle)?,
    );
    let files = crate::journal_source_inventory(&root, &request.source_sha, &git)?;
    let package = serde_json::json!({"name":"demo","version":"0.1.1","manifest":"Cargo.toml","tag":"demo-v0.1.1","owners":{"kind":"existing","owners":[{"id":1,"login":"owner","kind":"user"}]}});
    let semver = serde_json::json!({"demo":{"status":"no-baseline","checker_sha256":"bc9eb06ce04e97d6160c875ffdee1f6e56793e423dea025f344826be03e66829"}});
    let enrich = |receipt: &JournalReceipt| -> Result<JournalReceipt, Box<dyn std::error::Error>> {
        let mut value = serde_json::to_value(receipt)?;
        value["source_files"] = serde_json::to_value(&files)?;
        value["packages"] = serde_json::json!([package]);
        value["semver_evidence"] = semver.clone();
        Ok(serde_json::from_value(value)?)
    };
    let mut prior = enrich(prior)?;
    let forged_tool = std::env::var_os("VELNOR_TEST_FORGED_TOOL").is_some();
    if forged_tool {
        prior.expected_tools.generator_sha256 = "0".repeat(64);
    }
    let current = enrich(current)?;
    let journal = crate::crate_release_journal_adapter::JournalAdapter::new(
        crate::crate_release_github::NativeGitHubAdmissionRead::new("fixture".into())?,
    );
    seed_started_record(&journal, &prior.identity, &prior)?;
    crate::crate_release_journal::record_journal_intent(
        &journal,
        &prior.identity,
        &prior,
        &JournalIntendedMutation {
            sequence: 1,
            package: "demo".into(),
            version: "0.1.1".into(),
            kind: JournalMutationKind::Upload,
            semver_evidence: prior.semver_evidence.get("demo").cloned(),
        },
    )?;
    let edge = crate::crate_release_journal::PriorTransition { request, github };
    let result = crate::crate_release_journal::inspect_prior_chain_with(
        &journal,
        &current,
        &root,
        &git,
        Some(&edge),
    );
    if forged_tool {
        assert!(
            matches!(result, Err(crate::OrchestratorError::Internal { problem }) if problem == "crate_release_historical_tools_invalid"),
            "same-base forged historical tool did not fail its signed-tool check"
        );
        return Ok(());
    }
    let chain = result?;
    assert_eq!(chain.len(), 1);
    assert_eq!(
        chain[0].mutations.len(),
        1,
        "pending upload must remain visible to recovery"
    );
    let completion_ref = format!(
        "refs/tags/velnor-receipt/{}/{}/reconciled",
        current.identity.run_id, current.identity.run_attempt
    );
    let completion_before = journal.get_ref("owner/project", &completion_ref)?;
    let blob = journal
        .create_blob("owner/project", b"invalid changed predecessor history")
        .map_err(|error| format!("create drift blob: {error}"))?;
    let name = format!(
        "velnor-receipt/{}/{}/operation-2",
        prior.identity.run_id, prior.identity.run_attempt
    );
    let tag = JournalTag {
        name: name.clone(),
        object_type: "blob".into(),
        object_sha: blob,
        message: "velnor-release-receipt-v1".into(),
    };
    let tag_sha = journal
        .create_tag_object("owner/project", &tag)
        .map_err(|error| format!("create drift tag: {error}"))?;
    journal
        .create_ref("owner/project", &format!("refs/tags/{name}"), &tag_sha)
        .map_err(|error| format!("create drift ref: {error}"))?;
    assert!(
        crate::crate_release_journal::inspect_prior_chain_with(
            &journal,
            &current,
            &root,
            &git,
            Some(&edge),
        )
        .is_err(),
        "a valid signed predecessor with changed operation history must be rejected"
    );
    assert_eq!(
        journal.get_ref("owner/project", &completion_ref)?,
        completion_before,
        "a rejected predecessor must not change the completion ref"
    );
    Ok(())
}
