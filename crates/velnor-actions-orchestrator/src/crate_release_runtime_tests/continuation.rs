// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[cfg(unix)]
pub(super) fn verify_test_previous_continuation(
    github: &crate::crate_release_github::NativeGitHubAdmissionRead,
) -> Result<(), Box<dyn std::error::Error>> {
    let a = "a".repeat(40);
    let b = "b".repeat(40);
    let mut intent: velnor_actions_contract::config::CrateReleaseIntent = serde_json::from_value(
        serde_json::json!({"schema":1,"repository":"owner/project","default_branch":"main","environment":"release","required_workflow_path":".github/workflows/ci.yml","trusted_reviewer_id":139_017,"generator_version":"0.1.72","generator_source_sha":"c".repeat(40),"generator_manifest_sha256":"d".repeat(64),"release_plz_version":"0.4.3","runner":"ubuntu-26.04","authentication":"trusted-publishing","rust_version":"1.97.1","manifest_path":"Cargo.toml","pr_write_paths":["Cargo.toml"],"registry":"crates-io","packages":["demo"],"publish_features":{"demo":[]},"tag_template":"{{ package }}-v{{ version }}","journal_predecessor":{"run_id":11,"run_attempt":1,"workflow_sha":"e".repeat(40)},"continuation_source_sha":a}),
    )?;
    intent.validate()?;
    let request = crate::crate_release_github::GitHubAdmissionRequest {
        repository: "owner/project".into(),
        default_branch: "main".into(),
        environment: "release".into(),
        trusted_reviewer_id: 139_017,
        workflow_path: ".github/workflows/release.yml".into(),
        required_workflow_path: ".github/workflows/ci.yml".into(),
        workflow_sha: b.clone(),
        run_id: 12,
        run_attempt: 1,
        event: "workflow_dispatch".into(),
        source_sha: a.clone(),
        recovery_sha: Some(a.clone()),
    };
    let receipt: crate::crate_release_journal::JournalReceipt = serde_json::from_value(
        serde_json::json!({"identity":{"repository":"owner/project","workflow_sha":b,"source_sha":a,"run_id":12,"run_attempt":1},"predecessor":null,"intent_sha256":"c".repeat(64),"base_intent_sha256":"c".repeat(64),"registry":"crates-io","generator_version":"0.1.72","release_plz_version":"0.4.3","rust_version":"1.97.1","expected_tools":{"generator_sha256":"1".repeat(64),"release_plz_sha256":"2".repeat(64),"rust_manifest_sha256":"3".repeat(64)},"authentication":"trusted-publishing","required_check_id":1,"trusted_reviewer_id":139_017,"workflow_run_url":"https://github.com/owner/project/actions/runs/12","source_files":[],"packages":[],"semver_evidence":{}}),
    )?;
    crate::crate_release_github::verify_predecessor_authority(github, &request, &receipt, &intent)?;
    let mut recovery = intent.clone();
    recovery.continuation_source_sha = None;
    recovery.recovery = Some(velnor_actions_contract::config::CrateReleaseRecovery {
        package: "demo".into(),
        version: "0.1.1".into(),
        source_sha: a.clone(),
    });
    recovery.validate()?;
    crate::crate_release_github::verify_predecessor_authority(
        github, &request, &receipt, &recovery,
    )?;
    let mut creation = intent.clone();
    creation.continuation_source_sha = None;
    creation.journal_predecessor = None;
    creation.authentication =
        velnor_actions_contract::config::ReleaseAuthentication::BootstrapToken;
    creation.new_crates = Some(velnor_actions_contract::config::NewCrateRelease {
        source_sha: a.clone(),
        packages: std::collections::BTreeMap::from([("demo".into(), "0.1.1".into())]),
    });
    creation.validate()?;
    crate::crate_release_github::verify_predecessor_authority(
        github, &request, &receipt, &creation,
    )?;
    intent.continuation_source_sha = Some("c".repeat(40));
    assert!(
        crate::crate_release_github::verify_predecessor_authority(
            github, &request, &receipt, &intent
        )
        .is_err()
    );
    Ok(())
}

#[cfg(unix)]
pub(super) fn verify_real_journal_continuation(
    journal: &crate::crate_release_journal_adapter::JournalAdapter<
        crate::crate_release_github::NativeGitHubAdmissionRead,
    >,
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::crate_release_journal::{
        JournalIo as _, JournalTag, fixture_storage::seed_started_record, inspect_prior_history,
    };
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    let source = initialize_journal_source(root)?;
    let git = crate::crate_release_git::ReleaseGit::host_fixture();
    let files = crate::journal_source_inventory(root, &source, &git)?;
    let prior: crate::crate_release_journal::JournalReceipt = serde_json::from_value(
        serde_json::json!({
            "identity":{"repository":"owner/project","workflow_sha":source,"source_sha":source,"run_id":99,"run_attempt":1},
            "predecessor":null,"intent_sha256":"c".repeat(64),"base_intent_sha256":"c".repeat(64),
            "registry":"crates-io","generator_version":"0.1.72","release_plz_version":"0.4.3","rust_version":"1.97.1",
            "expected_tools":{"generator_sha256":"1".repeat(64),"release_plz_sha256":"2".repeat(64),"rust_manifest_sha256":"3".repeat(64)},
            "authentication":"trusted-publishing","required_check_id":1,"trusted_reviewer_id":139_017,
            "workflow_run_url":"https://github.com/owner/project/actions/runs/99","source_files":files,
            "packages":[{"name":"demo","version":"0.1.1","manifest":"Cargo.toml","tag":"demo-v0.1.1","owners":{"kind":"existing","owners":[{"id":1,"login":"owner","kind":"user"}]}}],
            "semver_evidence":{"demo":{"status":"no-baseline","checker_sha256":"bc9eb06ce04e97d6160c875ffdee1f6e56793e423dea025f344826be03e66829"}}
        }),
    )?;
    seed_started_record(journal, &prior.identity, &prior)?;
    crate::crate_release_journal::record_journal_intent(
        journal,
        &prior.identity,
        &prior,
        &crate::crate_release_journal::JournalIntendedMutation {
            sequence: 1,
            package: "demo".into(),
            version: "0.1.1".into(),
            kind: crate::crate_release_journal::JournalMutationKind::Upload,
            semver_evidence: prior.semver_evidence.get("demo").cloned(),
        },
    )?;
    let mut current = prior.clone();
    current.predecessor = Some(prior.identity.clone());
    current.identity.workflow_sha = "b".repeat(40);
    current.identity.run_id = 100;
    current.workflow_run_url = "https://github.com/owner/project/actions/runs/100".into();
    let observed = inspect_prior_history(journal, &prior.identity, &current, root, &git)?;
    assert_eq!(observed.start.receipt, prior);
    assert_eq!(observed.mutations.len(), 1);
    let mut wrong = current.clone();
    wrong.identity.source_sha = "c".repeat(40);
    assert!(inspect_prior_history(journal, &prior.identity, &wrong, root, &git).is_err());
    let name = "velnor-receipt/99/1/operation-2";
    let blob = journal.create_blob("owner/project", b"invalid changed history")?;
    let tag = JournalTag {
        name: name.into(),
        object_type: "blob".into(),
        object_sha: blob,
        message: "velnor-release-receipt-v1".into(),
    };
    let sha = journal.create_tag_object("owner/project", &tag)?;
    journal.create_ref("owner/project", &format!("refs/tags/{name}"), &sha)?;
    assert!(inspect_prior_history(journal, &prior.identity, &current, root, &git).is_err());
    assert!(
        journal
            .get_ref(
                "owner/project",
                "refs/tags/velnor-receipt/100/1/publication-started"
            )?
            .is_none()
    );
    Ok(())
}

#[cfg(unix)]
fn initialize_journal_source(root: &std::path::Path) -> Result<String, Box<dyn std::error::Error>> {
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname='demo'\nversion='0.1.1'\nedition='2021'\n",
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
            "-q",
            "-m",
            "approved source",
        ][..],
    ] {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(root)
            .output()?;
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let output = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(root)
        .output()?;
    let source = String::from_utf8(output.stdout)?.trim().to_owned();
    Ok(source)
}
