// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{Command, CrateReleaseIntent, GitHubAdmissionRequest, fs, git};
use std::error::Error;
use std::os::unix::fs::symlink;
use std::path::Path;

pub(crate) fn exercise() -> Result<(), Box<dyn Error>> {
    for bad_recovery in 0..=2 {
        exercise_case("workflow_dispatch", bad_recovery)?;
    }
    exercise_case("push", 0)?;
    Ok(())
}

fn exercise_case(event: &'static str, bad_recovery: u8) -> Result<(), Box<dyn Error>> {
    let fixture = super::historical_authority::signed_historical_fixture(event, bad_recovery)?;
    git(fixture.workflow.path(), &["config", "user.name", "Fixture"])?;
    git(
        fixture.workflow.path(),
        &["config", "user.email", "fixture@example.invalid"],
    )?;
    git(
        fixture.workflow.path(),
        &["config", "commit.gpgsign", "false"],
    )?;
    let previous = fixture.previous_intent.clone();
    let prior = fixture.prior_sha.clone();
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
    let source = fixture.workflow.path().join("release-source");
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
    let runner = tempfile::tempdir()?;
    let bundle = runner.path().join("velnor/git-bundle");
    fs::create_dir_all(&bundle)?;
    fs::copy("/tools/pinned-git", bundle.join("git"))?;
    fs::copy(
        "/tools/pinned-git-remote-http",
        bundle.join("git-remote-http"),
    )?;
    symlink("git-remote-http", bundle.join("git-remote-https"))?;
    velnor_actions_mise::release_git_pin::verify_bundle(&bundle)?;
    let output = Command::new(std::env::current_exe()?)
        .args([
            "--exact",
            "release::crate_release_runtime::tests::test_only_dispatch_and_loopback_transport",
        ])
        .env("VELNOR_TEST_CONTROLLER_SIGNED_CHILD", "1")
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
        .env("RUNNER_TEMP", runner.path())
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

pub(crate) fn run_child() -> Result<(), Box<dyn Error>> {
    use crate::crate_release_journal::{
        fixture_storage::seed_started_record, record_journal_intent,
    };
    use crate::release::package_release_signer::AdmittedStart;
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
    let source = workspace.join("release-source");
    let scratch = tempfile::tempdir()?;
    let (_, _, mut github, registry) = super::make_fixture(scratch.path())?;
    github.sha.clone_from(&request.source_sha);
    github.policy_sha.clone_from(&request.workflow_sha);
    github.approved.set(true);
    let initial = super::prepare_crate_release(&intent, &request, &github, &registry, &source)?;
    let (_, mut prior) = super::fixture_journal_plan(&source, &previous, &request, &initial)?;
    prior.identity.run_id = 11;
    prior.identity.workflow_sha.clone_from(&prior_sha);
    prior.workflow_run_url = "https://github.com/example/demo/actions/runs/11".into();
    prior
        .generator_version
        .clone_from(&previous.generator_version);
    prior
        .release_plz_version
        .clone_from(&previous.release_plz_version);
    prior.rust_version.clone_from(&previous.rust_version);
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
    let journal = super::JournalRegistryFake::default();
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
        older: None,
        event,
        defect: std::cell::Cell::new(0),
    };
    let admission = super::prepare_crate_release(&intent, &request, &auth, &registry, &source)?;
    let (generator_dir, generator) = verified_generator()?;
    let input = AdmittedStart {
        source_execution: &crate::UnprivilegedSourceExecution,
        git: super::test_git(),
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
    super::historical_authority::check_historical_authority(&input, event, bad_recovery)?;
    if event == "workflow_dispatch" && bad_recovery == 0 {
        verify_real_tool_measurement(&input, generator_dir.path())?;
        verify_prior_observation(&input, &prior)?;
        verify_frozen_predecessor_drift(&input, &prior)?;
    }
    Ok(())
}

fn verify_frozen_predecessor_drift(
    input: &crate::release::package_release_signer::AdmittedStart<
        '_,
        super::JournalRegistryFake,
        super::HistoricalAuthority<'_>,
        super::Registry,
    >,
    prior: &crate::crate_release_journal::JournalReceipt,
) -> Result<(), Box<dyn Error>> {
    use crate::crate_release_controller::{CrateReleaseObservation, FrozenCompletion};
    use crate::crate_release_journal::{JournalStage, inspect_journal};
    let (identity, _) =
        super::fixture_journal_plan(input.source, input.intent, input.request, input.admission)?;
    let receipt = inspect_journal(input.journal, &identity, JournalStage::Started)?
        .ok_or("signed current start missing")?
        .receipt;
    *input.registry.archive.borrow_mut() = super::fixture_archive(input.source)?;
    input.github.base.tag_exists.set(true);
    input.github.base.release_variant.set(0);
    let observation = CrateReleaseObservation {
        source_execution: input.source_execution,
        git: input.git,
        journal: input.journal,
        identity: &identity,
        journal_receipt: &receipt,
        admission: input.admission,
        intent: input.intent,
        request: input.request,
        github: input.github,
        registry: input.registry,
        source: input.source,
        sequence: 1,
    };
    let freeze = || -> Result<_, Box<dyn Error>> {
        let tools = crate::release::package_release_signer::measure_release_tools(
            &std::path::PathBuf::from(std::env::var("RUNNER_TEMP")?),
            input.intent,
            &receipt,
            input.generator,
        )?;
        Ok(FrozenCompletion::prepare_unprivileged(
            &observation,
            tools,
            &crate::UnprivilegedSourceExecution,
        )?)
    };
    let original_refs = input.journal.refs.borrow().clone();
    freeze()?.record_unprivileged(
        &crate::UnprivilegedSourceExecution,
        input.journal,
        input.github,
        input.registry,
    )?;
    assert!(inspect_journal(input.journal, &identity, JournalStage::Completed)?.is_some());
    *input.journal.refs.borrow_mut() = original_refs;
    for scenario in 0..3 {
        let frozen = freeze()?;
        let refs = input.journal.refs.borrow().clone();
        let writes = input.journal.write_calls.get();
        let prefix = format!(
            "refs/tags/velnor-receipt/{}/{}",
            prior.identity.run_id, prior.identity.run_attempt
        );
        match scenario {
            0 => {
                input
                    .journal
                    .refs
                    .borrow_mut()
                    .remove(&format!("{prefix}/publication-started"));
            }
            1 => {
                input
                    .journal
                    .refs
                    .borrow_mut()
                    .insert(format!("{prefix}/operation-1"), "a".repeat(40));
            }
            _ => {
                input
                    .journal
                    .refs
                    .borrow_mut()
                    .insert(format!("{prefix}/reconciled"), "a".repeat(40));
            }
        }
        let result = frozen.record_unprivileged(
            &crate::UnprivilegedSourceExecution,
            input.journal,
            input.github,
            input.registry,
        );
        *input.journal.refs.borrow_mut() = refs;
        assert!(result.is_err(), "post-freeze predecessor drift {scenario}");
        assert_eq!(input.journal.write_calls.get(), writes);
        assert!(inspect_journal(input.journal, &identity, JournalStage::Completed)?.is_none());
    }
    Ok(())
}

fn verify_real_tool_measurement(
    input: &crate::release::package_release_signer::AdmittedStart<
        '_,
        super::JournalRegistryFake,
        super::HistoricalAuthority<'_>,
        super::Registry,
    >,
    generator_dir: &Path,
) -> Result<(), Box<dyn Error>> {
    use crate::crate_release_journal::{JournalStage, inspect_journal};
    let (identity, _) =
        super::fixture_journal_plan(input.source, input.intent, input.request, input.admission)?;
    let receipt = inspect_journal(input.journal, &identity, JournalStage::Started)?
        .ok_or("signed current start missing")?
        .receipt;
    let runner = std::path::PathBuf::from(std::env::var("RUNNER_TEMP")?);
    for (source, relative) in [
        (
            generator_dir.join("velnor-actions-0.1.5-x86_64-unknown-linux-gnu"),
            "velnor/bootstrap/velnor-actions-0.1.5-x86_64-unknown-linux-gnu",
        ),
        (
            "/tools/signed-controller/release-plz".into(),
            "velnor-release-plz/release-plz",
        ),
        (
            "/tools/signed-controller/channel.toml".into(),
            "velnor/rust-manifest/channel.toml",
        ),
    ] {
        let target = runner.join(relative);
        fs::create_dir_all(target.parent().ok_or("tool parent missing")?)?;
        fs::copy(source, target)?;
    }
    crate::release::package_release_signer::measure_release_tools(
        &runner,
        input.intent,
        &receipt,
        input.generator,
    )?;
    Ok(())
}

fn verify_prior_observation(
    input: &crate::release::package_release_signer::AdmittedStart<
        '_,
        super::JournalRegistryFake,
        super::HistoricalAuthority<'_>,
        super::Registry,
    >,
    prior: &crate::crate_release_journal::JournalReceipt,
) -> Result<(), Box<dyn Error>> {
    use crate::crate_release_controller::{
        CrateReleaseObservation, RecoveryState, observe_prior_recovery,
    };
    use crate::crate_release_journal::{JournalStage, inspect_journal};
    let (identity, _) =
        super::fixture_journal_plan(input.source, input.intent, input.request, input.admission)?;
    let current = inspect_journal(input.journal, &identity, JournalStage::Started)?
        .ok_or("signed current start missing")?
        .receipt;
    let observation = CrateReleaseObservation {
        source_execution: input.source_execution,
        git: input.git,
        journal: input.journal,
        identity: &identity,
        journal_receipt: &current,
        admission: input.admission,
        intent: input.intent,
        request: input.request,
        github: input.github,
        registry: input.registry,
        source: input.source,
        sequence: 2,
    };
    let before = input.journal.write_calls.get();
    let missing = observe_prior_recovery(&observation, &prior.identity)?;
    assert_eq!(missing.len(), 1);
    assert_eq!(missing[0].state, RecoveryState::Missing);
    assert_eq!(
        serde_json::to_value(&missing[0])?["prior_record"],
        "intention-only"
    );
    assert_eq!(input.journal.write_calls.get(), before);
    *input.registry.archive.borrow_mut() = super::fixture_archive(input.source)?;
    input.registry.absent.set(false);
    let present = observe_prior_recovery(&observation, &prior.identity)?;
    assert_eq!(present[0].state, RecoveryState::Present);
    assert_eq!(input.journal.write_calls.get(), before);
    super::prior_observation::append_fixture_prior_upload_observation(input.journal, prior)?;
    let after_seed = input.journal.write_calls.get();
    let recorded = observe_prior_recovery(&observation, &prior.identity)?;
    assert_eq!(recorded[0].state, RecoveryState::Present);
    assert_eq!(
        serde_json::to_value(&recorded[0])?["prior_record"],
        "observation-recorded"
    );
    assert_eq!(input.journal.write_calls.get(), after_seed);
    input.github.base.approved.set(false);
    assert!(observe_prior_recovery(&observation, &prior.identity).is_err());
    input.github.base.approved.set(true);
    input.registry.archive.borrow_mut().push(0);
    assert!(observe_prior_recovery(&observation, &prior.identity).is_err());
    assert_eq!(input.journal.write_calls.get(), after_seed);
    Ok(())
}

fn verified_generator() -> Result<
    (
        tempfile::TempDir,
        crate::release::package_release_signer::AttestedLinuxGenerator,
    ),
    Box<dyn Error>,
> {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/attestation_fixtures");
    let temp = tempfile::tempdir()?;
    let binary = "velnor-actions-0.1.5-x86_64-unknown-linux-gnu";
    for name in [
        "release-manifest.json",
        "release-manifest.json.intoto.jsonl",
        "velnor-actions-0.1.5-x86_64-unknown-linux-gnu.intoto.jsonl",
    ] {
        fs::copy(source.join(name), temp.path().join(name))?;
    }
    let mut decoder = flate2::read::GzDecoder::new(fs::File::open(source.join("binary.gz"))?);
    std::io::copy(
        &mut decoder,
        &mut fs::File::create(temp.path().join(binary))?,
    )?;
    let deadline = velnor_actions_mise::CheckDeadline::after(std::time::Duration::from_secs(180))?;
    let (_, _, generator) =
        crate::release::package_release_signer::verified_generator_fixture(temp.path(), deadline)?;
    Ok((temp, generator))
}
