// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{Command, CrateReleaseIntent, GitHubAdmissionRequest, fs, git};
use std::error::Error;
use std::os::unix::fs::symlink;
use std::path::Path;

pub(crate) fn exercise() -> Result<(), Box<dyn Error>> {
    for bad_recovery in 0..=2 {
        exercise_case("workflow_dispatch", bad_recovery, false)?;
    }
    exercise_case("push", 0, false)?;
    exercise_case("workflow_dispatch", 0, true)?;
    creation::exercise()?;
    workspace::exercise()?;
    Ok(())
}

fn exercise_case(
    event: &'static str,
    bad_recovery: u8,
    two_link: bool,
) -> Result<(), Box<dyn Error>> {
    let fixture = super::historical_authority::signed_historical_fixture(event, bad_recovery)?;
    configure_fixture_git(fixture.workflow.path())?;
    let (previous, prior, older) = recovery::prepare_chain(&fixture, two_link)?;
    let mut intent = fixture.intent.clone();
    intent.generator_version = "0.1.5".into();
    intent.journal_predecessor = Some(velnor_actions_contract::config::CrateReleasePredecessor {
        run_id: 11,
        run_attempt: 1,
        workflow_sha: prior.clone(),
    });
    let policy = super::commit_history_intent(fixture.workflow.path(), &intent, "signed current")?;
    let mut request = fixture.request.clone();
    request.workflow_sha.clone_from(&policy);
    let runner = tempfile::tempdir()?;
    let source = runner.path().join("velnor-crate-release-source");
    let status = Command::new("git")
        .args(["clone", "--quiet", "--no-hardlinks", "--local"])
        .arg(fixture.workflow.path())
        .arg(&source)
        .status()?;
    if !status.success() {
        return Err("controller source clone failed".into());
    }
    git(
        &source,
        &["checkout", "--quiet", "--detach", &request.source_sha],
    )?;
    prepare_test_git_bundle(runner.path())?;
    let output = Command::new(std::env::current_exe()?)
        .args([
            "--exact",
            "release::crate_release_runtime::tests::test_only_dispatch_and_loopback_transport",
        ])
        .env("VELNOR_TEST_CONTROLLER_SIGNED_CHILD", "1")
        .env_remove("VELNOR_CONTROLLER_RECOVERY_CASE")
        .env("VELNOR_CONTROLLER_OLDER", serde_json::to_string(&older)?)
        .env("VELNOR_CONTROLLER_INTENT", serde_json::to_string(&intent)?)
        .env(
            "VELNOR_CONTROLLER_PREVIOUS",
            serde_json::to_string(&previous)?,
        )
        .env(
            "VELNOR_CONTROLLER_REQUEST",
            serde_json::to_string(&request)?,
        )
        .env("VELNOR_CONTROLLER_PRIOR", &prior)
        .env("VELNOR_CONTROLLER_BAD_RECOVERY", bad_recovery.to_string())
        .env("VELNOR_CONTROLLER_PRIOR_EVENT", event)
        .env("GITHUB_WORKSPACE", fixture.workflow.path())
        .env("RUNNER_TEMP", runner.path().canonicalize()?)
        .env("GITHUB_REPOSITORY", &request.repository)
        .env("GITHUB_EVENT_NAME", &request.event)
        .env("GITHUB_REF", "refs/heads/main")
        .env(
            "GITHUB_WORKFLOW_REF",
            format!(
                "{}/{}@refs/heads/main",
                request.repository, request.workflow_path
            ),
        )
        .env("GITHUB_WORKFLOW_SHA", &policy)
        .env("GITHUB_RUN_ID", request.run_id.to_string())
        .env("GITHUB_RUN_ATTEMPT", request.run_attempt.to_string())
        .output()?;
    if !output.status.success() {
        return Err(format!(
            "signed controller child failed: {} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    crate::libtest_case::assert_one_passed(&output);
    Ok(())
}

fn configure_fixture_git(path: &Path) -> Result<(), Box<dyn Error>> {
    git(path, &["config", "user.name", "Fixture"])?;
    git(path, &["config", "user.email", "fixture@example.invalid"])?;
    git(path, &["config", "commit.gpgsign", "false"])?;
    Ok(())
}

fn prepare_test_git_bundle(runner: &Path) -> Result<(), Box<dyn Error>> {
    let bundle = runner.join("velnor/git-bundle");
    fs::create_dir_all(&bundle)?;
    fs::copy("/tools/pinned-git", bundle.join("git"))?;
    fs::copy(
        "/tools/pinned-git-remote-http",
        bundle.join("git-remote-http"),
    )?;
    symlink("git-remote-http", bundle.join("git-remote-https"))?;
    velnor_actions_mise::release_git_pin::verify_bundle(&bundle)?;
    Ok(())
}

pub(crate) fn run_child() -> Result<(), Box<dyn Error>> {
    use crate::crate_release_journal::{
        fixture_storage::seed_started_record, record_journal_intent,
    };
    use crate::release::package_release_signer::AdmittedStart;
    match std::env::var("VELNOR_CONTROLLER_RECOVERY_CASE") {
        Ok(case) if case == "creation" => return creation::run_child(),
        Ok(case) if case == "workspace" => return workspace::run_child(),
        Err(std::env::VarError::NotPresent) => {}
        _ => return Err("invalid signed recovery case".into()),
    }
    let intent: CrateReleaseIntent =
        serde_json::from_str(&std::env::var("VELNOR_CONTROLLER_INTENT")?)?;
    let previous: CrateReleaseIntent =
        serde_json::from_str(&std::env::var("VELNOR_CONTROLLER_PREVIOUS")?)?;
    let request: GitHubAdmissionRequest =
        serde_json::from_str(&std::env::var("VELNOR_CONTROLLER_REQUEST")?)?;
    let prior_sha = std::env::var("VELNOR_CONTROLLER_PRIOR")?;
    let bad_recovery: u8 = std::env::var("VELNOR_CONTROLLER_BAD_RECOVERY")?.parse()?;
    let event = match std::env::var("VELNOR_CONTROLLER_PRIOR_EVENT")?.as_str() {
        "push" => "push",
        "workflow_dispatch" => "workflow_dispatch",
        _ => return Err("invalid historical event".into()),
    };
    let workspace = std::path::PathBuf::from(std::env::var("GITHUB_WORKSPACE")?);
    let source =
        std::path::PathBuf::from(std::env::var("RUNNER_TEMP")?).join("velnor-crate-release-source");
    let scratch = tempfile::tempdir()?;
    let (_, _, mut github, registry) = super::make_fixture(scratch.path())?;
    github.sha.clone_from(&request.source_sha);
    github.policy_sha.clone_from(&request.workflow_sha);
    github.approved.set(true);
    let initial = super::prepare_crate_release(&intent, &request, &github, &registry, &source)?;
    let mut prior = prior_receipt(&source, &previous, &request, &initial, &prior_sha)?;
    let journal = super::JournalRegistryFake::default();
    let older = recovery::seed_older(&journal, &mut prior, &request, &initial, &source)?;
    seed_started_record(&journal, &prior.identity, &prior)?;
    record_journal_intent(
        &journal,
        &prior.identity,
        &prior,
        &super::fixture_upload_intention(),
    )?;
    let auth = super::HistoricalAuthority {
        base: &github,
        workflow: prior_sha,
        older,
        event,
        defect: std::cell::Cell::new(0),
    };
    let admission = super::prepare_crate_release(&intent, &request, &auth, &registry, &source)?;
    let (generator_dir, generator) = verified_generator()?;
    let runner = std::path::PathBuf::from(std::env::var("RUNNER_TEMP")?);
    let git = crate::crate_release_git::ReleaseGit::new(
        velnor_actions_mise::release_git_pin::verify_bundle(&runner.join("velnor/git-bundle"))?,
    );
    let input = AdmittedStart {
        source_execution: &crate::UnprivilegedSourceExecution,
        git: &git,
        journal: &journal,
        github: &auth,
        registry: &registry,
        intent: &intent,
        request: &request,
        admission: &admission,
        source: &source,
        workflow_root: &workspace,
        generator: &generator,
    };
    if input.github.older.is_some() {
        return recovery::verify_two_link(&input, &prior);
    }
    super::historical_authority::check_historical_authority(&input, event, bad_recovery)?;
    if event == "workflow_dispatch" && bad_recovery == 0 {
        verify_real_tool_measurement(&input, generator_dir.path())?;
        verify_prior_observation(&input, &prior)?;
        verify_frozen_predecessor_drift(&input, &prior)?;
    }
    Ok(())
}

fn prior_receipt(
    source: &Path,
    previous: &CrateReleaseIntent,
    request: &GitHubAdmissionRequest,
    initial: &crate::crate_release_controller::CrateReleaseAdmissionReceipt,
    prior_sha: &str,
) -> Result<crate::crate_release_journal::JournalReceipt, Box<dyn Error>> {
    let (_, mut prior) = super::fixture_journal_plan(source, previous, request, initial)?;
    prior.identity.run_id = 11;
    prior_sha.clone_into(&mut prior.identity.workflow_sha);
    prior.workflow_run_url = "https://github.com/example/demo/actions/runs/11".into();
    prior
        .generator_version
        .clone_from(&previous.generator_version);
    prior
        .release_plz_version
        .clone_from(&previous.release_plz_version);
    prior.rust_version.clone_from(&previous.rust_version);
    pin_previous_tools(&mut prior, previous)?;
    Ok(prior)
}

fn pin_previous_tools(
    prior: &mut crate::crate_release_journal::JournalReceipt,
    previous: &CrateReleaseIntent,
) -> Result<(), Box<dyn Error>> {
    prior.expected_tools.generator_sha256 =
        "040c647a575684b84f28588862bad78876faf0224d8e5538c7a680d7d528f6b3".into();
    prior.expected_tools.release_plz_sha256 = velnor_actions_mise::catalog::qualified_release_plz(
        &previous.release_plz_version,
        &previous.runner,
    )
    .ok_or("release-plz pin")?
    .executable_sha256
    .into();
    prior.expected_tools.rust_manifest_sha256 =
        velnor_actions_mise::catalog::rust_channel_manifest_sha256(&previous.rust_version)
            .ok_or("rust pin")?
            .into();
    Ok(())
}

#[path = "crate_release_controller_signed_history_verify.rs"]
mod verify;
use verify::*;

#[path = "crate_release_controller_signed_history_creation.rs"]
mod creation;
#[path = "crate_release_controller_signed_history_recovery.rs"]
mod recovery;
#[path = "crate_release_controller_signed_history_workspace.rs"]
mod workspace;
