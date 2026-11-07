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
    #[cfg(not(target_os = "linux"))]
    verify_missing_history_context(&receipt, &request, &intent);
    crate::crate_release_github::verify_predecessor_authority(github, &request, &receipt, &intent)?;
    let mut wrong_workflow = request.clone();
    wrong_workflow.workflow_path = ".github/workflows/not-release.yml".into();
    assert!(
        crate::crate_release_github::verify_predecessor_authority(
            github,
            &wrong_workflow,
            &receipt,
            &intent
        )
        .is_err()
    );
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

#[cfg(all(unix, not(target_os = "linux")))]
fn verify_missing_history_context(
    receipt: &crate::crate_release_journal::JournalReceipt,
    request: &crate::crate_release_github::GitHubAdmissionRequest,
    intent: &velnor_actions_contract::config::CrateReleaseIntent,
) {
    let git = crate::crate_release_git::ReleaseGit::host_fixture();
    let deadline = velnor_actions_mise::CheckDeadline::after(std::time::Duration::from_secs(30))
        .expect("history test deadline");
    let result = crate::release::crate_release_direct_history::HistoryContext::admitted(
        std::path::Path::new("/missing-history-source"),
        intent,
        request,
        &git,
        receipt,
        deadline,
    );
    assert!(matches!(
        result,
        Err(crate::OrchestratorError::Internal { problem })
            if problem == "crate_release_history_custody_required"
    ));
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

#[cfg(target_os = "linux")]
pub(super) fn verify_real_direct_history(
    github: &crate::crate_release_github::NativeGitHubAdmissionRead,
) -> Result<(), Box<dyn std::error::Error>> {
    verify_real_direct_history_transition(
        github,
        std::env::var_os("VELNOR_TEST_UPDATED_HISTORY").is_none(),
    )
}

#[cfg(target_os = "linux")]
fn verify_real_direct_history_transition(
    github: &crate::crate_release_github::NativeGitHubAdmissionRead,
    old_runner: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::crate_release_github::GitHubAdmissionRead as _;
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    let source = initialize_journal_source(root)?;
    let mut intent = direct_history_fixture_intent(&source)?;
    intent.validate()?;
    std::fs::create_dir(root.join(".github"))?;
    let mut historical = intent.clone();
    historical.generator_version = "0.1.5".into();
    historical.generator_source_sha = "f1041f322c54cd7edfdef06afd307799eeaa801a".into();
    historical.generator_manifest_sha256 =
        "e38d047150f9e0347ec2160fd65bb7f026842e6f416e456990cf3b7ccd284c52".into();
    if old_runner && std::env::var_os("VELNOR_TEST_SAME_BASE_SIGNED").is_none() {
        historical.runner = "ubuntu-24.04".into();
    }
    let old_policy = write_policy_commit(root, &historical, "old release policy")?;
    intent.journal_predecessor = Some(velnor_actions_contract::config::CrateReleasePredecessor {
        run_id: 11,
        run_attempt: 1,
        workflow_sha: old_policy.clone(),
    });
    intent.validate()?;
    let policy = write_policy_commit(root, &intent, "release policy")?;
    github.get(&format!("repos/owner/project/compare/{source}...{policy}"))?;
    let request = crate::crate_release_github::GitHubAdmissionRequest {
        repository: "owner/project".into(),
        default_branch: "main".into(),
        environment: "release".into(),
        trusted_reviewer_id: 139_017,
        workflow_path: ".github/workflows/release.yml".into(),
        required_workflow_path: ".github/workflows/ci.yml".into(),
        workflow_sha: policy.clone(),
        run_id: 12,
        run_attempt: 1,
        event: "workflow_dispatch".into(),
        source_sha: source.clone(),
        recovery_sha: Some(source.clone()),
    };
    let receipt = direct_fixture_receipt(&request, &old_policy, &intent)?;
    let git = crate::crate_release_git::ReleaseGit::host_fixture();
    let deadline = velnor_actions_mise::CheckDeadline::after(std::time::Duration::from_secs(30))?;
    assert_eq!(
        crate::release::crate_release_historical_intent::verify_prior_bound(
            root, &receipt, &request, github, &git, deadline
        )?,
        intent
    );
    let mut forged = receipt.clone();
    forged.intent_sha256 = "0".repeat(64);
    assert!(
        crate::release::crate_release_historical_intent::verify_prior_bound(
            root, &forged, &request, github, &git, deadline
        )
        .is_err()
    );
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        let mut prior = receipt.clone();
        prior.identity.workflow_sha.clone_from(&old_policy);
        prior.identity.run_id = 11;
        prior.predecessor = None;
        prior.workflow_run_url = "https://github.com/owner/project/actions/runs/11".into();
        prior
            .generator_version
            .clone_from(&historical.generator_version);
        prior.expected_tools.generator_sha256 =
            "040c647a575684b84f28588862bad78876faf0224d8e5538c7a680d7d528f6b3".into();
        prior.expected_tools.release_plz_sha256 =
            velnor_actions_mise::catalog::qualified_release_plz(
                &historical.release_plz_version,
                &historical.runner,
            )
            .ok_or("historical release-plz pin")?
            .executable_sha256
            .into();
        prior.intent_sha256 = crate::planner::generator::sha256_hex(
            &velnor_actions_contract::canonical_json_bytes(&historical)?,
        );
        prior.base_intent_sha256 =
            crate::crate_release_journal::release_base_intent_sha256(&historical)?;
        direct_adapter::exercise(root, &policy, &source, &request, &receipt, &prior)?;
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn direct_history_fixture_intent(
    source: &str,
) -> Result<velnor_actions_contract::config::CrateReleaseIntent, Box<dyn std::error::Error>> {
    let updated = std::env::var_os("VELNOR_TEST_UPDATED_HISTORY").is_some();
    Ok(serde_json::from_value(serde_json::json!({
        "schema":1,"repository":"owner/project","default_branch":"main","environment":"release",
        "required_workflow_path":".github/workflows/ci.yml","trusted_reviewer_id":139_017,
        "generator_version":if updated { "0.1.73" } else { "0.1.5" },"generator_source_sha":if updated { "82ad7e9f788718855469444a3a02ea79ba0fe087" } else { "f1041f322c54cd7edfdef06afd307799eeaa801a" },"generator_manifest_sha256":if updated { "8e71b846acd375086fcb9693e567985d5a48b902e2e4b84400724ca6daad32d9" } else { "e38d047150f9e0347ec2160fd65bb7f026842e6f416e456990cf3b7ccd284c52" },
        "release_plz_version":"0.4.3","runner":"ubuntu-26.04","authentication":"trusted-publishing",
        "rust_version":"1.97.1","manifest_path":"Cargo.toml","pr_write_paths":["Cargo.toml"],
        "registry":"crates-io","packages":["demo"],"publish_features":{"demo":[]},
        "tag_template":"{{ package }}-v{{ version }}",
        "recovery":{"package":"demo","version":"0.1.1","source_sha":source}
    }))?)
}

#[cfg(target_os = "linux")]
fn direct_fixture_receipt(
    request: &crate::crate_release_github::GitHubAdmissionRequest,
    old_policy: &str,
    intent: &velnor_actions_contract::config::CrateReleaseIntent,
) -> Result<crate::crate_release_journal::JournalReceipt, Box<dyn std::error::Error>> {
    let digest = crate::planner::generator::sha256_hex(
        &velnor_actions_contract::canonical_json_bytes(intent)?,
    );
    let base = crate::crate_release_journal::release_base_intent_sha256(intent)?;
    Ok(serde_json::from_value(serde_json::json!({
        "identity":{"repository":"owner/project","workflow_sha":request.workflow_sha,"source_sha":request.source_sha,"run_id":12,"run_attempt":1},
        "predecessor":{"repository":"owner/project","workflow_sha":old_policy,"source_sha":request.source_sha,"run_id":11,"run_attempt":1},
        "intent_sha256":digest,"base_intent_sha256":base,"registry":"crates-io",
        "generator_version":intent.generator_version,"release_plz_version":"0.4.3","rust_version":"1.97.1",
        "expected_tools":{"generator_sha256":if intent.generator_version == "0.1.73" { "e1b5224cedc462623de15056e75e76499245cb035bc70d20a2ca1a478bb68cd5" } else { "040c647a575684b84f28588862bad78876faf0224d8e5538c7a680d7d528f6b3" },"release_plz_sha256":velnor_actions_mise::catalog::qualified_release_plz("0.4.3", "ubuntu-26.04").ok_or("release-plz pin")?.executable_sha256,"rust_manifest_sha256":velnor_actions_mise::catalog::rust_channel_manifest_sha256("1.97.1").ok_or("rust pin")?},
        "authentication":"trusted-publishing","required_check_id":1,"trusted_reviewer_id":139_017,
        "workflow_run_url":"https://github.com/owner/project/actions/runs/12","source_files":[],"packages":[],"semver_evidence":{}
    }))?)
}

#[cfg(target_os = "linux")]
fn write_policy_commit(
    root: &std::path::Path,
    intent: &velnor_actions_contract::config::CrateReleaseIntent,
    message: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    std::fs::write(
        root.join(".github/velnor-release-intent.toml"),
        toml::to_string(intent)?,
    )?;
    for args in [
        &["add", "."][..],
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "-qm",
            message,
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
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[path = "direct_history_adapter.rs"]
mod direct_adapter;
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
pub(super) use direct_adapter::verify_direct_history_child;
