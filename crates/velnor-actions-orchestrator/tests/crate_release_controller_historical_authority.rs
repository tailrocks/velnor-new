// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    Cell, Command, CrateReleaseIntent, GitHubAdmissionRequest, Github, HistoricalAuthority,
    JournalRegistryFake, Registry, fixture_journal_plan, fixture_no_baseline, fs, git,
    make_fixture, prepare_crate_release, test_git,
};

#[test]
pub(super) fn previous_push_or_dispatch_requires_original_historical_authority()
-> Result<(), Box<dyn std::error::Error>> {
    for event in ["push", "workflow_dispatch"] {
        exercise_historical_authority(event, 0)?;
    }
    exercise_historical_authority("workflow_dispatch", 1)?;
    exercise_historical_authority("workflow_dispatch", 2)?;
    Ok(())
}

pub(super) struct HistoricalFixture {
    pub(super) temp: tempfile::TempDir,
    pub(super) workflow: tempfile::TempDir,
    pub(super) intent: CrateReleaseIntent,
    pub(super) request: GitHubAdmissionRequest,
    pub(super) github: Github,
    pub(super) registry: Registry,
    pub(super) previous_intent: CrateReleaseIntent,
    pub(super) prior_sha: String,
}

pub(super) fn historical_fixture(
    event: &'static str,
    bad_recovery: u8,
) -> Result<HistoricalFixture, Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let source = temp.path();
    let (mut intent, mut request, mut github, registry) = make_fixture(source)?;
    github.approved.set(true);
    let mut previous_intent = intent.clone();
    if bad_recovery == 1 {
        previous_intent.recovery = None;
    }
    if bad_recovery == 2 {
        previous_intent
            .recovery
            .as_mut()
            .ok_or("recovery")?
            .source_sha = "e".repeat(40);
    }
    if event == "push" {
        std::fs::create_dir_all(source.join(".github"))?;
        fs::write(
            source.join(".github/velnor-release-intent.toml"),
            toml::to_string(&previous_intent)?,
        )?;
        git(source, &["add", "-A"])?;
        git(source, &["commit", "-q", "-s", "-m", "prior release"])?;
        let sha = git(source, &["rev-parse", "HEAD"])?;
        request.source_sha.clone_from(&sha);
        request.recovery_sha = Some(sha.clone());
        intent
            .recovery
            .as_mut()
            .ok_or("recovery")?
            .source_sha
            .clone_from(&sha);
        github.sha = sha;
    }
    let workflow = tempfile::tempdir()?;
    let output = Command::new("git")
        .args(["clone", "-q", "--no-hardlinks"])
        .arg(source)
        .arg(workflow.path())
        .output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    if event == "workflow_dispatch" {
        git(workflow.path(), &["config", "user.name", "Fixture"])?;
        git(
            workflow.path(),
            &["config", "user.email", "fixture@example.invalid"],
        )?;
        git(workflow.path(), &["config", "commit.gpgsign", "false"])?;
        fs::create_dir_all(workflow.path().join(".github"))?;
        fs::write(
            workflow.path().join(".github/velnor-release-intent.toml"),
            toml::to_string(&previous_intent)?,
        )?;
        git(workflow.path(), &["add", "-A"])?;
        git(
            workflow.path(),
            &["commit", "-q", "-s", "-m", "prior dispatch"],
        )?;
    }
    let prior_sha = git(workflow.path(), &["rev-parse", "HEAD"])?;
    assert_eq!(prior_sha == request.source_sha, event == "push");
    Ok(HistoricalFixture {
        temp,
        workflow,
        intent,
        request,
        github,
        registry,
        previous_intent,
        prior_sha,
    })
}

pub(super) fn exercise_historical_authority(
    event: &'static str,
    bad_recovery: u8,
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::crate_release_journal::{
        JournalIntendedMutation, JournalMutationKind, fixture_storage::seed_started_record,
        record_journal_intent,
    };
    use crate::release::package_release_signer::{AdmittedStart, fixture_attested_generator};
    let HistoricalFixture {
        temp,
        workflow,
        mut intent,
        request,
        github,
        registry,
        previous_intent,
        prior_sha,
    } = historical_fixture(event, bad_recovery)?;
    let source = temp.path();
    let observed = prepare_crate_release(&intent, &request, &github, &registry, source)?;
    let (_, mut prior) = fixture_journal_plan(source, &previous_intent, &request, &observed)?;
    prior.identity.run_id = 11;
    prior.identity.workflow_sha.clone_from(&prior_sha);
    prior.workflow_run_url = "https://github.com/example/demo/actions/runs/11".into();
    let journal = JournalRegistryFake::default();
    seed_started_record(&journal, &prior.identity, &prior)?;
    record_journal_intent(
        &journal,
        &prior.identity,
        &prior,
        &JournalIntendedMutation {
            semver_evidence: Some(fixture_no_baseline()),
            sequence: 1,
            package: "demo".into(),
            version: "0.1.1".into(),
            kind: JournalMutationKind::Upload,
        },
    )?;
    intent.journal_predecessor = Some(velnor_actions_contract::config::CrateReleasePredecessor {
        run_id: 11,
        run_attempt: 1,
        workflow_sha: prior_sha.clone(),
    });
    let admission = prepare_crate_release(&intent, &request, &github, &registry, source)?;
    let auth = HistoricalAuthority {
        base: &github,
        workflow: prior_sha,
        older: None,
        event,
        defect: Cell::new(0),
    };
    let generator = fixture_attested_generator();
    let input = AdmittedStart {
        source_execution: &crate::UnprivilegedSourceExecution,
        git: test_git(),
        journal: &journal,
        github: &auth,
        registry: &registry,
        intent: &intent,
        request: &request,
        admission: &admission,
        source,
        workflow_root: workflow.path(),
        generator: &generator,
    };
    check_historical_authority(&input, event, bad_recovery)?;
    Ok(())
}

pub(super) fn check_historical_authority(
    input: &crate::release::package_release_signer::AdmittedStart<
        '_,
        JournalRegistryFake,
        HistoricalAuthority<'_>,
        Registry,
    >,
    event: &str,
    bad_recovery: u8,
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::release::package_release_signer::record_admitted_start;
    for defect in 1..=if event == "push" { 9 } else { 8 } {
        input.github.defect.set(defect);
        let writes = input.journal.write_calls.get();
        assert!(
            record_admitted_start(input).is_err(),
            "accepted {event} historical defect {defect}"
        );
        assert_eq!(
            input.journal.write_calls.get(),
            writes,
            "wrote journal for defect {defect}"
        );
        assert_eq!(
            input.github.base.writes.get(),
            0,
            "wrote metadata for defect {defect}"
        );
    }
    input.github.defect.set(0);
    if bad_recovery != 0 {
        let writes = input.journal.write_calls.get();
        assert!(
            record_admitted_start(input).is_err(),
            "accepted unapproved historical recovery source"
        );
        assert_eq!(input.journal.write_calls.get(), writes);
        assert_eq!(input.github.base.writes.get(), 0);
    } else {
        input.github.defect.set(10);
        record_admitted_start(input)?;
        input.github.defect.set(0);
    }
    Ok(())
}

#[test]
pub(super) fn workflow_lock_uses_event_scoped_recovery_and_committed_policy()
-> Result<(), Box<dyn std::error::Error>> {
    let source = tempfile::tempdir()?;
    let policy = tempfile::tempdir()?;
    let (mut intent, mut request, github, registry) = make_fixture(source.path())?;
    intent.recovery.as_mut().expect("recovery").source_sha = "f".repeat(40);
    request.event = "push".into();
    request.recovery_sha = None;
    let effective = super::super::effective_release_intent(&intent, &request);
    let plan = crate::release::crate_release_workspace::crate_release_workspace_plan_with_registry(
        source.path(),
        &request.source_sha,
        &effective,
        &registry,
        test_git(),
        &crate::UnprivilegedSourceExecution,
    )?;
    git(policy.path(), &["init", "-q"])?;
    fs::create_dir_all(policy.path().join(".github/workflows"))?;
    let workflow = policy.path().join(".github/workflows/release.yml");
    fs::write(
        &workflow,
        format!(
            "name: release\nconcurrency:\n  group: {}\n  cancel-in-progress: false\n  queue: max\njobs: {{}}\n",
            plan.concurrency_group
        ),
    )?;
    git(policy.path(), &["add", "."])?;
    git(
        policy.path(),
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "-qm",
            "policy",
        ],
    )?;
    request.workflow_sha = git(policy.path(), &["rev-parse", "HEAD"])?;
    crate::release::crate_release_runtime::verify_workflow_concurrency(
        policy.path(),
        source.path(),
        &intent,
        &request,
        &registry,
        test_git(),
        &crate::UnprivilegedSourceExecution,
    )?;
    fs::write(source.path().join("src/lib.rs"), "pub fn modified() {}")?;
    assert!(
        crate::release::crate_release_runtime::verify_workflow_concurrency(
            policy.path(),
            source.path(),
            &intent,
            &request,
            &registry,
            test_git(),
            &crate::UnprivilegedSourceExecution,
        )
        .is_err()
    );
    fs::write(source.path().join("src/lib.rs"), "pub fn demo() {}")?;
    request.event = "workflow_dispatch".into();
    request.recovery_sha = Some("f".repeat(40));
    assert!(
        crate::release::crate_release_runtime::verify_workflow_concurrency(
            policy.path(),
            source.path(),
            &intent,
            &request,
            &registry,
            test_git(),
            &crate::UnprivilegedSourceExecution,
        )
        .is_err()
    );
    assert_eq!(github.writes.get(), 0);
    Ok(())
}
