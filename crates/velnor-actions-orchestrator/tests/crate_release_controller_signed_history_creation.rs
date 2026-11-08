// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::super::{Cell, HistoricalAuthority, JournalRegistryFake, NewRegistry};
use super::{Command, CrateReleaseIntent, Error, GitHubAdmissionRequest, Path, git};

pub(super) fn exercise() -> Result<(), Box<dyn Error>> {
    let fixture =
        super::super::historical_authority::signed_historical_fixture("workflow_dispatch", 0)?;
    super::configure_fixture_git(fixture.workflow.path())?;
    let mut previous = fixture.previous_intent.clone();
    previous.authentication =
        velnor_actions_contract::config::ReleaseAuthentication::BootstrapToken;
    previous.recovery = None;
    previous.new_crates = Some(velnor_actions_contract::config::NewCrateRelease {
        source_sha: fixture.request.source_sha.clone(),
        packages: std::collections::BTreeMap::from([("demo".into(), "0.1.1".into())]),
    });
    let prior = super::super::commit_history_intent(
        fixture.workflow.path(),
        &previous,
        "signed prior creation",
    )?;
    let mut intent = previous.clone();
    intent.journal_predecessor = Some(velnor_actions_contract::config::CrateReleasePredecessor {
        run_id: 11,
        run_attempt: 1,
        workflow_sha: prior.clone(),
    });
    let policy = super::super::commit_history_intent(
        fixture.workflow.path(),
        &intent,
        "signed creation recovery",
    )?;
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
        return Err("creation source clone failed".into());
    }
    git(
        &source,
        &["checkout", "--quiet", "--detach", &request.source_sha],
    )?;
    run_parent(
        runner.path(),
        fixture.workflow.path(),
        &intent,
        &previous,
        &request,
        &prior,
    )
}

fn run_parent(
    runner: &Path,
    workspace: &Path,
    intent: &CrateReleaseIntent,
    previous: &CrateReleaseIntent,
    request: &GitHubAdmissionRequest,
    prior: &str,
) -> Result<(), Box<dyn Error>> {
    super::prepare_test_git_bundle(runner)?;
    let output = Command::new(std::env::current_exe()?)
        .args([
            "--exact",
            "release::crate_release_runtime::tests::test_only_dispatch_and_loopback_transport",
        ])
        .env("VELNOR_TEST_CONTROLLER_SIGNED_CHILD", "1")
        .env("VELNOR_CONTROLLER_RECOVERY_CASE", "creation")
        .env("VELNOR_CONTROLLER_INTENT", serde_json::to_string(intent)?)
        .env(
            "VELNOR_CONTROLLER_PREVIOUS",
            serde_json::to_string(previous)?,
        )
        .env("VELNOR_CONTROLLER_REQUEST", serde_json::to_string(request)?)
        .env("VELNOR_CONTROLLER_PRIOR", prior)
        .env("GITHUB_WORKSPACE", workspace)
        .env("RUNNER_TEMP", runner.canonicalize()?)
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
        .env("GITHUB_WORKFLOW_SHA", &request.workflow_sha)
        .env("GITHUB_RUN_ID", request.run_id.to_string())
        .env("GITHUB_RUN_ATTEMPT", request.run_attempt.to_string())
        .output()?;
    if !output.status.success() {
        return Err(format!(
            "signed creation child failed: {} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    crate::libtest_case::assert_one_passed(&output);
    Ok(())
}

pub(super) fn run_child() -> Result<(), Box<dyn Error>> {
    use crate::release::package_release_signer::{AdmittedStart, record_admitted_start};
    let intent: CrateReleaseIntent =
        serde_json::from_str(&std::env::var("VELNOR_CONTROLLER_INTENT")?)?;
    let previous: CrateReleaseIntent =
        serde_json::from_str(&std::env::var("VELNOR_CONTROLLER_PREVIOUS")?)?;
    let request: GitHubAdmissionRequest =
        serde_json::from_str(&std::env::var("VELNOR_CONTROLLER_REQUEST")?)?;
    let workspace = std::path::PathBuf::from(std::env::var("GITHUB_WORKSPACE")?);
    let source =
        std::path::PathBuf::from(std::env::var("RUNNER_TEMP")?).join("velnor-crate-release-source");
    let scratch = tempfile::tempdir()?;
    let (_, _, mut github, base) = super::super::make_fixture(scratch.path())?;
    github.sha.clone_from(&request.source_sha);
    github.policy_sha.clone_from(&request.workflow_sha);
    github.approved.set(true);
    let registry = NewRegistry {
        base,
        visible: Cell::new(false),
        owner_changed: Cell::new(false),
    };
    let prior = prior_receipt(&source, &previous, &request, &github, &registry)?;
    let journal = JournalRegistryFake::default();
    crate::crate_release_journal::fixture_storage::seed_started_record(
        &journal,
        &prior.identity,
        &prior,
    )?;
    *registry.base.archive.borrow_mut() = super::super::fixture_archive(&source)?;
    registry.base.absent.set(false);
    registry.visible.set(true);
    let auth = HistoricalAuthority {
        base: &github,
        workflow: prior.identity.workflow_sha.clone(),
        older: None,
        event: "workflow_dispatch",
        defect: Cell::new(0),
    };
    let runner = std::path::PathBuf::from(std::env::var("RUNNER_TEMP")?);
    let git = crate::crate_release_git::ReleaseGit::new(
        velnor_actions_mise::release_git_pin::verify_bundle(&runner.join("velnor/git-bundle"))?,
    );
    let admission = creation_admission(&source, &intent, &request, &auth, &registry, &git)?;
    let (_generator_dir, generator) = super::verified_generator()?;
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
    super::super::reject_untrusted_prior_creation(&input, &prior)?;
    record_admitted_start(&input)?;
    super::super::verify_created_crate_recovers_without_reupload(&input)?;
    Ok(())
}

fn prior_receipt(
    source: &Path,
    previous: &CrateReleaseIntent,
    request: &GitHubAdmissionRequest,
    github: &super::super::Github,
    registry: &NewRegistry,
) -> Result<crate::crate_release_journal::JournalReceipt, Box<dyn Error>> {
    let admission =
        super::super::prepare_crate_release(previous, request, github, registry, source)?;
    let (_, mut prior) = super::super::fixture_journal_plan(source, previous, request, &admission)?;
    prior.identity.run_id = 11;
    prior.identity.workflow_sha = std::env::var("VELNOR_CONTROLLER_PRIOR")?;
    prior.workflow_run_url = "https://github.com/example/demo/actions/runs/11".into();
    prior
        .generator_version
        .clone_from(&previous.generator_version);
    prior
        .release_plz_version
        .clone_from(&previous.release_plz_version);
    prior.rust_version.clone_from(&previous.rust_version);
    prior.authentication = crate::crate_release_journal::JournalAuthentication::BootstrapToken;
    prior.packages[0].owners = crate::crate_release_journal::JournalOwnership::NewCrate;
    super::pin_previous_tools(&mut prior, previous)?;
    Ok(prior)
}

fn creation_admission(
    source: &Path,
    intent: &CrateReleaseIntent,
    request: &GitHubAdmissionRequest,
    github: &HistoricalAuthority<'_>,
    registry: &NewRegistry,
    git: &crate::crate_release_git::ReleaseGit,
) -> Result<crate::crate_release_controller::CrateReleaseAdmissionReceipt, Box<dyn Error>> {
    let mut admission =
        super::super::prepare_crate_release(intent, request, github, registry, source)?;
    let tools = tempfile::tempdir()?;
    admission.semver_evidence = crate::release::crate_release_semver::verify_preflight(
        source,
        &admission,
        registry,
        tools.path(),
        git,
        &crate::UnprivilegedSourceExecution,
    )?;
    assert!(matches!(
        admission.semver_evidence.get("demo"),
        Some(crate::release::crate_release_semver::SemverEvidence::MetadataOnly { .. })
    ));
    Ok(admission)
}
