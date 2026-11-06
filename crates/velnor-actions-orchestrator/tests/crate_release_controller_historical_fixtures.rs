// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    Cell, CrateReleaseIntent, CrateReleaseRepairWrite, GitHubAdmissionRead, Github,
    JournalRegistryFake, OrchestratorError, Path, Registry, Value, fixture_archive,
    fixture_journal_plan, fixture_no_baseline, fs, git, json, make_fixture, prepare_crate_release,
    test_git,
};

pub(super) fn initialize_history_repo(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    git(path, &["init", "-q"])?;
    git(path, &["config", "user.name", "Alexey Zhokhov"])?;
    git(path, &["config", "user.email", "alexey@zhokhov.com"])?;
    git(path, &["config", "commit.gpgsign", "false"])?;
    Ok(())
}

pub(super) fn commit_history_intent(
    path: &Path,
    intent: &CrateReleaseIntent,
    message: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    fs::create_dir_all(path.join(".github"))?;
    fs::write(
        path.join(".github/velnor-release-intent.toml"),
        toml::to_string(intent)?,
    )?;
    git(path, &["add", "-A"])?;
    git(path, &["commit", "-q", "-s", "-m", message])?;
    git(path, &["rev-parse", "HEAD"])
}

pub(super) fn assert_two_link_recovery(
    input: &crate::package_release_signer::AdmittedStart<
        '_,
        JournalRegistryFake,
        HistoricalAuthority<'_>,
        Registry,
    >,
    previous: &crate::crate_release_journal::JournalReceipt,
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::crate_release_controller::{CrateReleaseObservation, coordinate_release};
    use crate::crate_release_journal::{JournalStage, inspect_journal};
    let identity = crate::crate_release_journal::JournalIdentity {
        repository: input.request.repository.clone(),
        workflow_sha: input.request.workflow_sha.clone(),
        source_sha: input.request.source_sha.clone(),
        run_id: input.request.run_id,
        run_attempt: input.request.run_attempt,
    };
    let started =
        inspect_journal(input.journal, &identity, JournalStage::Started)?.ok_or("start")?;
    assert_eq!(started.receipt.predecessor, Some(previous.identity.clone()));
    let observation = CrateReleaseObservation {
        source_execution: &crate::UnprivilegedSourceExecution,
        git: test_git(),
        journal: input.journal,
        identity: &identity,
        journal_receipt: &started.receipt,
        admission: input.admission,
        intent: input.intent,
        request: input.request,
        github: input.github,
        registry: input.registry,
        source: input.source,
        sequence: 1,
    };
    let writes = input.journal.write_calls.get();
    let attempted = Cell::new(false);
    assert!(
        coordinate_release(
            &observation,
            |name| crate::crate_release_semver::fixture_verified_semver(input.admission, name),
            |_| {
                attempted.set(true);
                Err(OrchestratorError::Contract {
                    problem: "unexpected upload".into(),
                })
            },
            || {}
        )
        .is_err()
    );
    assert!(!attempted.get());
    assert_eq!(input.journal.write_calls.get(), writes);
    assert_eq!(input.github.base.writes.get(), 0);
    *input.registry.archive.borrow_mut() = fixture_archive(input.source)?;
    input.registry.absent.set(false);
    input.github.base.release_variant.set(4);
    let result = coordinate_release(
        &observation,
        |name| crate::crate_release_semver::fixture_verified_semver(input.admission, name),
        |_| {
            attempted.set(true);
            Err(OrchestratorError::Contract {
                problem: "unexpected upload".into(),
            })
        },
        || {},
    )?;
    assert!(!attempted.get());
    assert_eq!(result.packages.len(), 1);
    assert_eq!(input.github.base.writes.get(), 2);
    Ok(())
}

#[test]
pub(super) fn admitted_start_rejects_forged_prior_intent_digest()
-> Result<(), Box<dyn std::error::Error>> {
    use crate::crate_release_journal::{record_journal_intent, record_source_bound_start};
    use crate::package_release_signer::{
        AdmittedStart, fixture_attested_generator, record_admitted_start,
    };
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    let workflow = tempfile::tempdir()?;
    git(workflow.path(), &["init", "-q"])?;
    git(workflow.path(), &["config", "user.name", "Alexey Zhokhov"])?;
    git(
        workflow.path(),
        &["config", "user.email", "alexey@zhokhov.com"],
    )?;
    git(workflow.path(), &["config", "commit.gpgsign", "false"])?;
    let (mut intent, request, github, registry) = make_fixture(root)?;
    github.approved.set(true);
    let mut previous_intent = intent.clone();
    previous_intent.recovery = None;
    fs::create_dir_all(workflow.path().join(".github"))?;
    fs::write(
        workflow.path().join(".github/velnor-release-intent.toml"),
        toml::to_string(&previous_intent)?,
    )?;
    git(workflow.path(), &["add", "-A"])?;
    git(
        workflow.path(),
        &["commit", "-q", "-s", "-m", "previous workflow"],
    )?;
    let previous_workflow_sha = git(workflow.path(), &["rev-parse", "HEAD"])?;
    let original = prepare_crate_release(&intent, &request, &github, &registry, root)?;
    let (_, mut previous) = fixture_journal_plan(root, &previous_intent, &request, &original)?;
    previous.identity.run_id = 11;
    previous.identity.workflow_sha = previous_workflow_sha.clone();
    previous.workflow_run_url = "https://github.com/example/demo/actions/runs/11".into();
    let journal = JournalRegistryFake::default();
    previous.intent_sha256 = "d".repeat(64);
    record_source_bound_start(&journal, &previous.identity, &previous, root, test_git())?;
    record_journal_intent(
        &journal,
        &previous.identity,
        &previous,
        &fixture_upload_intention(),
    )?;
    intent.journal_predecessor = Some(velnor_actions_contract::config::CrateReleasePredecessor {
        run_id: 11,
        run_attempt: 1,
        workflow_sha: previous_workflow_sha,
    });
    let admission = prepare_crate_release(&intent, &request, &github, &registry, root)?;
    let generator = fixture_attested_generator();
    let input = AdmittedStart {
        source_execution: &crate::UnprivilegedSourceExecution,
        git: test_git(),
        journal: &journal,
        github: &github,
        registry: &registry,
        intent: &intent,
        request: &request,
        admission: &admission,
        source: root,
        workflow_root: workflow.path(),
        generator: &generator,
    };
    fs::write(
        workflow.path().join(".github/velnor-release-intent.toml"),
        "untrusted dirty file",
    )?;
    let before = journal.write_calls.get();
    assert!(
        record_admitted_start(&input).is_err(),
        "wrong prior intent digest must reject before writes"
    );
    assert_eq!(journal.write_calls.get(), before);
    Ok(())
}

pub(super) fn fixture_upload_intention() -> crate::crate_release_journal::JournalIntendedMutation {
    crate::crate_release_journal::JournalIntendedMutation {
        semver_evidence: Some(fixture_no_baseline()),
        sequence: 1,
        package: "demo".into(),
        version: "0.1.1".into(),
        kind: crate::crate_release_journal::JournalMutationKind::Upload,
    }
}

pub(super) struct HistoricalAuthority<'a> {
    pub(super) base: &'a Github,
    pub(super) workflow: String,
    pub(super) older: Option<String>,
    pub(super) event: &'static str,
    pub(super) defect: Cell<u8>,
}

impl CrateReleaseRepairWrite for HistoricalAuthority<'_> {
    fn create_tag(
        &self,
        repository: &str,
        tag: &str,
        source_sha: &str,
    ) -> Result<(), OrchestratorError> {
        self.base.create_tag(repository, tag, source_sha)
    }
    fn create_release(
        &self,
        repository: &str,
        tag: &str,
        source_sha: &str,
        version: &str,
    ) -> Result<(), OrchestratorError> {
        self.base
            .create_release(repository, tag, source_sha, version)
    }
}

impl GitHubAdmissionRead for HistoricalAuthority<'_> {
    fn get(&self, route: &str) -> Result<Value, OrchestratorError> {
        let defect = self.defect.get();
        let source = &self.base.sha;
        let old_run = route.contains("/actions/runs/10");
        let run_id = if old_run { 10 } else { 11 };
        let workflow = if old_run {
            self.older.as_ref().unwrap_or(&self.workflow)
        } else {
            &self.workflow
        };
        let check_sha = self
            .older
            .as_ref()
            .filter(|sha| route.contains(sha.as_str()))
            .unwrap_or(&self.workflow);
        let run = || json!({"id":if defect == 1 { 99 } else { run_id },"run_attempt":if defect == 2 { 2 } else { 1 },"status":"completed","head_sha":if defect == 3 { "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee" } else { workflow },"head_branch":"main","event":if defect == 4 { "schedule" } else { self.event },"path":if defect == 5 { ".github/workflows/other.yml" } else { ".github/workflows/release.yml" },"repository":{"id":if defect == 6 { 99 } else { 22 },"full_name":"example/demo","owner":{"id":77,"login":"example"}},"head_repository":{"id":22,"full_name":"example/demo","owner":{"id":77,"login":"example"}}});
        if route.ends_with("/actions/runs/11/attempts/1")
            || route.ends_with("/actions/runs/11")
            || route.ends_with("/actions/runs/10/attempts/1")
            || route.ends_with("/actions/runs/10")
        {
            return Ok(run());
        }
        if route.ends_with("/actions/runs/11/approvals")
            || route.ends_with("/actions/runs/10/approvals")
        {
            return Ok(if defect == 7 {
                json!([])
            } else {
                json!([{"state":"approved","user":{"type":"User","id":if defect == 8 { 1 } else { 139_017 }},"environments":[{"id":9,"name":"release"}],"comment":format!("I reviewed the complete release policy at {} and source {}, and authorize this source for run {}.", workflow, source, run_id)}])
            });
        }
        if (route.contains(&self.workflow)
            || self.older.as_ref().is_some_and(|sha| route.contains(sha)))
            && route.contains("check-runs?")
        {
            return Ok(
                json!({"total_count":1,"check_runs":[{"id":1,"name":"Required","head_sha":check_sha,"check_suite":{"id":123},"app":{"id":15368,"slug":"github-actions"},"status":"completed","conclusion":"success","started_at":"2026-11-03T15:40:00Z"}]}),
            );
        }
        if (route.contains(&self.workflow)
            || self.older.as_ref().is_some_and(|sha| route.contains(sha)))
            && route.contains("/actions/runs?")
        {
            return Ok(
                json!({"total_count":1,"workflow_runs":[{"check_suite_id":123,"head_sha":check_sha,"path":".github/workflows/ci.yml","status":"completed","conclusion":"success"}]}),
            );
        }
        if route.contains("/commits/") && route.contains("/pulls?") {
            return Ok(
                json!([{"number":7,"merged_at":"2026-11-03T15:38:50Z","merge_commit_sha":source,"user":{"login":"author"},"base":{"ref":"main","repo":{"full_name":"example/demo","owner":{"id":77,"login":"example"}}},"head":{"ref":"release-plz-test","sha":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","repo":{"full_name":"example/demo","owner":{"id":77,"login":"example"}}}}]),
            );
        }
        if route.contains("/git/commits/") {
            let sha = route.rsplit('/').next().unwrap_or_default();
            return Ok(
                json!({"sha":sha,"tree":{"sha":"cccccccccccccccccccccccccccccccccccccccc"}}),
            );
        }
        if route.contains("/pulls/7/reviews?") {
            return Ok(if defect == 9 {
                json!([])
            } else {
                json!([{"id":1,"user":{"login":"reviewer","id":55,"type":"User"},"author_association":"MEMBER","state":"APPROVED","commit_id":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","submitted_at":"2026-11-03T15:00:00Z"}])
            });
        }
        if route.ends_with("/collaborators/reviewer/permission") {
            return Ok(json!({"user":{"id":55,"type":"User"},"permission":"write"}));
        }
        self.base.get(route)
    }
    fn get_optional(&self, route: &str) -> Result<Option<Value>, OrchestratorError> {
        self.base.get_optional(route)
    }
}
