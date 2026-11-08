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
#[path = "crate_release_controller_signed_history_workspace_helpers.rs"]
mod helpers;
use helpers::{
    attach_semver_evidence, coordinate_continuation, make_mixed, prepare_prior,
    reject_forged_predecessors, run_signed_child,
};

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
    run_signed_child(
        &intent,
        &previous,
        &request,
        &prior,
        &policy,
        workflow.path(),
        runner.path(),
    )?;
    Ok(())
}

pub(super) fn run_child() -> Result<(), Box<dyn Error>> {
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
    let (mut mixed, registry) = make_mixed(&request)?;
    let git = crate::crate_release_git::ReleaseGit::new(
        velnor_actions_mise::release_git_pin::verify_bundle(&runner.join("velnor/git-bundle"))?,
    );
    let (prior, prior_upload, first_archive) = prepare_prior(
        &previous, &request, &prior_sha, &source, &git, &mut mixed, &registry,
    )?;
    let store = super::super::JournalRegistryFake::default();
    let journal =
        crate::crate_release_journal_adapter::JournalAdapter::new(CustodyTransport(&store));
    seed_started_record(&journal, &prior.identity, &prior)
        .map_err(|error| format!("seed prior start: {error}"))?;
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
    attach_semver_evidence(&mut admission, &source, &registry, &git)?;
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
    reject_forged_predecessors(&input, &prior, &prior_upload, &store)?;
    record_admitted_start(&input).map_err(|error| format!("admitted start: {error}"))?;
    let (identity, _) = super::super::fixture_journal_plan(&source, &intent, &request, &admission)?;
    let receipt = inspect_journal(&journal, &identity, JournalStage::Started)?
        .ok_or("current start missing")?
        .receipt;
    assert_eq!(receipt.predecessor.as_ref(), Some(&prior.identity));
    coordinate_continuation(&input, &identity, &receipt, first_archive)?;
    Ok(())
}
