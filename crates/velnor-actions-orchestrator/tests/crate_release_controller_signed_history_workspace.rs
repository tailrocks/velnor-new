// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::OrchestratorError;
use crate::crate_release_controller::CrateReleaseRepairWrite;
use crate::crate_release_github::GitHubAdmissionRead;
use serde_json::Value;

#[path = "crate_release_controller_signed_history_custody.rs"]
mod custody;
use custody::CustodyTransport;

struct HistoricalMixed<'a> {
    history: super::super::HistoricalAuthority<'a>,
    mixed: &'a super::super::MixedGithub,
}

impl GitHubAdmissionRead for HistoricalMixed<'_> {
    fn get(&self, route: &str) -> Result<Value, OrchestratorError> {
        self.history.get(route)
    }
    fn get_optional(&self, route: &str) -> Result<Option<Value>, OrchestratorError> {
        if route.contains("velnor_fixture_order_20261104_") {
            self.mixed.get_optional(route)
        } else {
            self.history.get_optional(route)
        }
    }
}

impl CrateReleaseRepairWrite for HistoricalMixed<'_> {
    fn create_tag(
        &self,
        repository: &str,
        tag: &str,
        source_sha: &str,
    ) -> Result<(), OrchestratorError> {
        self.mixed.create_tag(repository, tag, source_sha)
    }
    fn create_release(
        &self,
        repository: &str,
        tag: &str,
        source_sha: &str,
        version: &str,
    ) -> Result<(), OrchestratorError> {
        self.mixed
            .create_release(repository, tag, source_sha, version)
    }
}

#[test]
#[ignore = "requires qualified Linux Git, Rust, signed generator and isolated GitHub transport"]
pub(super) fn exercise() -> Result<(), Box<dyn Error>> {
    let (temp, mut previous, mut request, mut github, _) =
        super::super::make_workspace_order_fixture()?;
    previous.generator_version = "0.1.5".into();
    let source = temp.path();
    fs::create_dir_all(source.join(".github"))?;
    fs::write(
        source.join(".github/velnor-release-intent.toml"),
        toml::to_string(&previous)?,
    )?;
    git(source, &["add", "-A"])?;
    git(source, &["commit", "-qm", "reviewed prior release"])?;
    let prior = git(source, &["rev-parse", "HEAD"])?;
    github.0.sha.clone_from(&prior);
    github.0.policy_sha.clone_from(&prior);
    request.source_sha.clone_from(&prior);
    request.workflow_sha.clone_from(&prior);
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
    super::configure_fixture_git(workflow.path())?;
    let mut intent = previous.clone();
    intent.journal_predecessor = Some(velnor_actions_contract::config::CrateReleasePredecessor {
        run_id: 11,
        run_attempt: 1,
        workflow_sha: prior.clone(),
    });
    intent.continuation_source_sha = Some(prior.clone());
    intent.validate()?;
    let policy = super::super::commit_history_intent(
        workflow.path(),
        &intent,
        "reviewed workspace continuation",
    )?;
    request.event = "workflow_dispatch".into();
    request.workflow_sha = policy.clone();
    request.recovery_sha = Some(prior.clone());
    let runner = tempfile::tempdir()?;
    let checkout = runner.path().join("velnor-crate-release-source");
    let output = Command::new("git")
        .args(["clone", "-q", "--no-hardlinks", "--local"])
        .arg(workflow.path())
        .arg(&checkout)
        .output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    git(&checkout, &["checkout", "-q", "--detach", &prior])?;
    super::prepare_test_git_bundle(runner.path())?;
    let output = Command::new(std::env::current_exe()?)
        .args([
            "--exact",
            "release::crate_release_runtime::tests::test_only_dispatch_and_loopback_transport",
        ])
        .env("VELNOR_TEST_CONTROLLER_SIGNED_CHILD", "1")
        .env("VELNOR_CONTROLLER_RECOVERY_CASE", "workspace")
        .env("GH_TOKEN", "fixture-token")
        .env(
            "VELNOR_TEST_ATTESTATION_ROOT",
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/attestation_fixtures/historical/trusted-root.jsonl"),
        )
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
        .env("GITHUB_WORKSPACE", workflow.path())
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
    assert!(
        output.status.success(),
        "signed workspace child failed: {} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    crate::libtest_case::assert_one_passed(&output);
    Ok(())
}

pub(super) fn run_child() -> Result<(), Box<dyn Error>> {
    use crate::crate_release_controller::{
        CrateReleaseObservation, UploadAttempt, coordinate_release,
    };
    use crate::crate_release_journal::{
        JournalStage, fixture_storage::seed_started_record, inspect_journal, record_journal_intent,
    };
    use crate::release::package_release_signer::{AdmittedStart, record_admitted_start};
    use std::cell::{Cell, RefCell};
    let intent: CrateReleaseIntent =
        serde_json::from_str(&std::env::var("VELNOR_CONTROLLER_INTENT")?)?;
    let previous: CrateReleaseIntent =
        serde_json::from_str(&std::env::var("VELNOR_CONTROLLER_PREVIOUS")?)?;
    let request: GitHubAdmissionRequest =
        serde_json::from_str(&std::env::var("VELNOR_CONTROLLER_REQUEST")?)?;
    let prior_sha = std::env::var("VELNOR_CONTROLLER_PRIOR")?;
    let runner = std::path::PathBuf::from(std::env::var("RUNNER_TEMP")?);
    let source = runner.join("velnor-crate-release-source");
    let workflow = std::path::PathBuf::from(std::env::var("GITHUB_WORKSPACE")?);
    let (_, _, _, mut workspace_github, registry) = super::super::make_workspace_order_fixture()?;
    workspace_github.0.sha.clone_from(&request.source_sha);
    workspace_github
        .0
        .policy_sha
        .clone_from(&request.workflow_sha);
    workspace_github.0.push_event.set(false);
    let mut mixed = super::super::MixedGithub {
        inner: workspace_github,
        tags: [Cell::new(false), Cell::new(false)],
        releases: [Cell::new(false), Cell::new(false)],
        writes: RefCell::new(Vec::new()),
    };
    let git = crate::crate_release_git::ReleaseGit::new(
        velnor_actions_mise::release_git_pin::verify_bundle(&runner.join("velnor/git-bundle"))?,
    );
    let mut prior_request = request.clone();
    prior_request.event = "push".into();
    prior_request.workflow_sha.clone_from(&prior_sha);
    prior_request.recovery_sha = None;
    mixed.inner.0.push_event.set(true);
    mixed.inner.0.policy_sha.clone_from(&prior_sha);
    let original = super::super::prepare_crate_release(
        &previous,
        &prior_request,
        &mixed.inner,
        &registry,
        &source,
    )?;
    let target = tempfile::tempdir()?;
    let first_archive = crate::crate_release_controller::package_unprivileged(
        &source,
        &previous,
        &original.packages[0].source,
        target.path(),
        &request.source_sha,
        false,
        &git,
    )?;
    let (_, mut prior) =
        super::super::fixture_journal_plan(&source, &previous, &prior_request, &original)?;
    super::super::mixed_workspace::set_workspace_journal_packages(&mut prior, &original);
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
    super::pin_previous_tools(&mut prior, &previous)?;
    let store = super::super::JournalRegistryFake::default();
    let journal =
        crate::crate_release_journal_adapter::JournalAdapter::new(CustodyTransport(&store));
    seed_started_record(&journal, &prior.identity, &prior)
        .map_err(|error| format!("seed prior start: {error}"))?;
    let mut prior_upload = super::super::fixture_upload_intention();
    prior_upload.package = original.packages[0].source.name.clone();
    record_journal_intent(&journal, &prior.identity, &prior, &prior_upload)
        .map_err(|error| format!("seed prior upload: {error}"))?;
    *registry.archive.borrow_mut() = first_archive.clone();
    registry.visible.set(true);
    mixed.inner.0.push_event.set(false);
    mixed.inner.0.policy_sha.clone_from(&request.workflow_sha);
    let github = HistoricalMixed {
        history: super::super::HistoricalAuthority {
            base: &mixed.inner.0,
            workflow: prior_sha,
            older: None,
            event: "push",
            defect: Cell::new(0),
        },
        mixed: &mixed,
    };
    let registry = super::super::MixedRegistry {
        first: registry,
        second: RefCell::new(None),
    };
    let mut admission =
        super::super::prepare_crate_release(&intent, &request, &github, &registry, &source)
            .map_err(|error| format!("current admission: {error}"))?;
    let evidence = crate::release::crate_release_semver::verify_one(
        &source,
        &admission,
        &registry,
        &admission.packages[0],
        Path::new("/unused-checker"),
        &git,
        &crate::UnprivilegedSourceExecution,
    )?;
    admission
        .semver_evidence
        .insert(admission.packages[0].source.name.clone(), evidence);
    admission.semver_evidence.insert(
        admission.packages[1].source.name.clone(),
        crate::release::crate_release_semver::SemverEvidence::Deferred {
            predecessors: vec![admission.packages[0].source.name.clone()],
        },
    );
    let (_generator_dir, generator) = super::verified_generator()?;
    let input = AdmittedStart {
        source_execution: &crate::UnprivilegedSourceExecution,
        git: &git,
        journal: &journal,
        github: &github,
        registry: &registry,
        intent: &intent,
        request: &request,
        admission: &admission,
        source: &source,
        workflow_root: &workflow,
        generator: &generator,
    };
    let before = store.write_calls.get();
    let mut wrong_source = request.clone();
    wrong_source.source_sha = "f".repeat(40);
    let wrong = AdmittedStart {
        request: &wrong_source,
        ..input
    };
    assert!(record_admitted_start(&wrong).is_err());
    assert_eq!(store.write_calls.get(), before);
    assert!(mixed.writes.borrow().is_empty());
    assert!(registry.second.borrow().is_none());
    for scenario in ["source", "selection"] {
        let forged_store = super::super::JournalRegistryFake::default();
        let forged_journal = crate::crate_release_journal_adapter::JournalAdapter::new(
            CustodyTransport(&forged_store),
        );
        let mut forged = prior.clone();
        if scenario == "source" {
            forged.identity.source_sha = "f".repeat(40);
        } else {
            let removed = forged.packages.pop().ok_or("missing second package")?;
            forged.semver_evidence.remove(&removed.name);
        }
        seed_started_record(&forged_journal, &forged.identity, &forged)
            .map_err(|error| format!("seed forged {scenario} start: {error}"))?;
        record_journal_intent(&forged_journal, &forged.identity, &forged, &prior_upload)
            .map_err(|error| format!("seed forged {scenario} upload: {error}"))?;
        let before = forged_store.write_calls.get();
        let wrong = AdmittedStart {
            journal: &forged_journal,
            ..input
        };
        assert!(
            record_admitted_start(&wrong).is_err(),
            "accepted forged {scenario}"
        );
        assert_eq!(
            forged_store.write_calls.get(),
            before,
            "wrote journal after forged {scenario}"
        );
        assert!(mixed.writes.borrow().is_empty());
        assert!(registry.second.borrow().is_none());
    }
    record_admitted_start(&input).map_err(|error| format!("admitted start: {error}"))?;
    let (identity, _) = super::super::fixture_journal_plan(&source, &intent, &request, &admission)?;
    let receipt = inspect_journal(&journal, &identity, JournalStage::Started)?
        .ok_or("current start missing")?
        .receipt;
    assert_eq!(receipt.predecessor.as_ref(), Some(&prior.identity));
    let server =
        super::super::SparseFixture::start("velnor_fixture_order_20261104_a", first_archive)?;
    super::super::FIXTURE_SPARSE_INDEX.with(|value| *value.borrow_mut() = Some(server.url.clone()));
    let waits = Cell::new(0);
    let observation = CrateReleaseObservation {
        source_execution: &crate::UnprivilegedSourceExecution,
        git: &git,
        journal: &journal,
        identity: &identity,
        journal_receipt: &receipt,
        admission: &admission,
        intent: &intent,
        request: &request,
        github: &github,
        registry: &registry,
        source: &source,
        sequence: 1,
    };
    let result = coordinate_release(
        &observation,
        |name| {
            super::super::mixed_workspace::mixed_verified_semver(&admission, &mixed, &server, name)
        },
        |permit| {
            assert_eq!(permit.package, "velnor_fixture_order_20261104_b");
            assert_eq!(&*mixed.writes.borrow(), &["tag-0", "release-0"]);
            let target = tempfile::tempdir().map_err(|_| OrchestratorError::Contract {
                problem: "fixture".into(),
            })?;
            let archive = crate::crate_release_controller::package_unprivileged(
                &source,
                &intent,
                &admission.packages[1].source,
                target.path(),
                &request.source_sha,
                false,
                &git,
            )?;
            *registry.second.borrow_mut() = Some(archive);
            Ok(UploadAttempt::ExitedSuccess)
        },
        || super::super::mixed_workspace::propagate_sparse_index(&registry, &server, &waits),
    );
    super::super::FIXTURE_SPARSE_INDEX.with(|value| *value.borrow_mut() = None);
    assert_eq!(waits.get(), 1);
    super::super::mixed_workspace::check_mixed_workspace_result(&mixed, result?.packages.len());
    Ok(())
}
