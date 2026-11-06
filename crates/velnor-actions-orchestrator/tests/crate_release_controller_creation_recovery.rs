// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    Cell, CrateRegistryRead, CrateRegistryResponse, CrateReleaseRepairWrite, GitHubAdmissionRead,
    Github, HistoricalAuthority, JournalRegistryFake, MixedGithub, MixedRegistry, NewRegistry,
    OrchestratorError, Value, fixture_archive, fixture_journal_plan, fixture_no_baseline, json,
    make_fixture, prepare_crate_release, test_git,
};

pub(super) fn verify_created_crate_recovers_without_reupload(
    input: &crate::package_release_signer::AdmittedStart<
        '_,
        JournalRegistryFake,
        HistoricalAuthority<'_>,
        NewRegistry,
    >,
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::crate_release_controller::{CrateReleaseObservation, coordinate_release};
    use crate::crate_release_journal::{JournalStage, inspect_journal};
    let request = input.request;
    let journal = input.journal;
    let admission = input.admission;
    let intent = input.intent;
    let auth = input.github;
    let registry = input.registry;
    let root = input.source;
    let github = auth.base;
    let identity = crate::crate_release_journal::JournalIdentity {
        repository: request.repository.clone(),
        workflow_sha: request.workflow_sha.clone(),
        source_sha: request.source_sha.clone(),
        run_id: request.run_id,
        run_attempt: request.run_attempt,
    };
    let started = inspect_journal(journal, &identity, JournalStage::Started)?.ok_or("start")?;
    let observation = CrateReleaseObservation {
        source_execution: &crate::UnprivilegedSourceExecution,
        git: test_git(),
        journal,
        identity: &identity,
        journal_receipt: &started.receipt,
        admission,
        intent,
        request,
        github: auth,
        registry,
        source: root,
        sequence: 1,
    };
    github.release_variant.set(4);
    let attempted = Cell::new(false);
    let result = coordinate_release(
        &observation,
        |name| crate::crate_release_semver::fixture_verified_semver(admission, name),
        |_| {
            attempted.set(true);
            Err(OrchestratorError::Contract {
                problem: "must not reupload".into(),
            })
        },
        || {},
    )?;
    assert!(!attempted.get());
    assert_eq!(result.packages.len(), 1);
    Ok(())
}

pub(super) fn reject_untrusted_prior_creation(
    input: &crate::package_release_signer::AdmittedStart<
        '_,
        JournalRegistryFake,
        HistoricalAuthority<'_>,
        NewRegistry,
    >,
    prior: &crate::crate_release_journal::JournalReceipt,
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::crate_release_journal::{
        JournalIntendedMutation, JournalMutationKind, record_journal_intent,
    };
    use crate::package_release_signer::record_admitted_start;
    let journal = input.journal;
    let auth = input.github;
    let registry = input.registry;
    let github = auth.base;
    let before = journal.write_calls.get();
    assert!(record_admitted_start(input).is_err());
    assert_eq!(journal.write_calls.get(), before);
    record_journal_intent(
        journal,
        &prior.identity,
        prior,
        &JournalIntendedMutation {
            sequence: 1,
            package: "demo".into(),
            version: "0.1.1".into(),
            kind: JournalMutationKind::Upload,
            semver_evidence: Some(fixture_no_baseline()),
        },
    )?;
    auth.defect.set(7);
    let before = journal.write_calls.get();
    assert!(record_admitted_start(input).is_err());
    assert_eq!(journal.write_calls.get(), before);
    auth.defect.set(0);
    github.owner_match.set(false);
    assert!(record_admitted_start(input).is_err());
    assert_eq!(journal.write_calls.get(), before);
    github.owner_match.set(true);
    let original_archive = registry.base.archive.borrow().clone();
    registry.base.archive.borrow_mut().push(0);
    assert!(record_admitted_start(input).is_err());
    assert_eq!(journal.write_calls.get(), before);
    *registry.base.archive.borrow_mut() = original_archive;
    Ok(())
}

#[test]
pub(super) fn coordinator_completes_first_crate_with_durable_owners()
-> Result<(), Box<dyn std::error::Error>> {
    use crate::crate_release_controller::{CrateReleaseObservation, coordinate_release};
    use crate::crate_release_journal::{
        JournalAuthentication, JournalOwnership, record_source_bound_start,
    };
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    let (mut intent, request, github, base) = make_fixture(root)?;
    intent.authentication = velnor_actions_contract::config::ReleaseAuthentication::BootstrapToken;
    intent.recovery = None;
    intent.new_crates = Some(velnor_actions_contract::config::NewCrateRelease {
        source_sha: request.source_sha.clone(),
        packages: std::collections::BTreeMap::from([("demo".into(), "0.1.1".into())]),
    });
    github.approved.set(true);
    github.release_variant.set(4);
    let registry = NewRegistry {
        base,
        visible: Cell::new(false),
        owner_changed: Cell::new(false),
    };
    *registry.base.archive.borrow_mut() = fixture_archive(root)?;
    let admission = prepare_crate_release(&intent, &request, &github, &registry, root)?;
    let (identity, mut plan) = fixture_journal_plan(root, &intent, &request, &admission)?;
    plan.authentication = JournalAuthentication::BootstrapToken;
    plan.packages[0].owners = JournalOwnership::NewCrate;
    let journal = JournalRegistryFake::default();
    record_source_bound_start(&journal, &identity, &plan, root, test_git())?;
    let input = CrateReleaseObservation {
        source_execution: &crate::UnprivilegedSourceExecution,
        git: test_git(),
        journal: &journal,
        identity: &identity,
        journal_receipt: &plan,
        admission: &admission,
        intent: &intent,
        request: &request,
        github: &github,
        registry: &registry,
        source: root,
        sequence: 1,
    };
    let uploads = Cell::new(0);
    let result = coordinate_release(
        &input,
        |name| crate::crate_release_semver::fixture_verified_semver(&admission, name),
        |_| {
            uploads.set(uploads.get() + 1);
            registry.base.absent.set(false);
            registry.visible.set(true);
            Ok(super::super::observation::UploadAttempt::ExitedSuccess)
        },
        || {},
    )?;
    assert_eq!(uploads.get(), 1);
    assert_eq!(github.writes.get(), 2);
    assert_eq!(result.packages.len(), 1);
    let tools = crate::package_release_signer::fixture_measured_tools(plan.expected_tools.clone());
    crate::crate_release_journal::record_unprivileged_completion(&input, tools)?;
    let completed = crate::crate_release_journal::inspect_journal(
        &journal,
        &identity,
        crate::crate_release_journal::JournalStage::Completed,
    )?
    .ok_or("missing completion")?
    .completion
    .ok_or("missing completion data")?;
    assert_eq!(completed.packages[0].owners.len(), 1);
    assert_eq!(completed.packages[0].owners[0].id, 370_532);
    assert_eq!(completed.packages[0].owners[0].login, "owner");
    let writes = journal.write_calls.get();
    let tools = crate::package_release_signer::fixture_measured_tools(plan.expected_tools.clone());
    crate::crate_release_journal::record_unprivileged_completion(&input, tools)?;
    assert_eq!(journal.write_calls.get(), writes);
    assert_eq!(github.writes.get(), 2);
    reject_changed_first_crate_completion(&input, writes);
    Ok(())
}

pub(super) fn reject_changed_first_crate_completion(
    input: &crate::crate_release_controller::CrateReleaseObservation<
        '_,
        JournalRegistryFake,
        Github,
        NewRegistry,
    >,
    writes: usize,
) {
    input.registry.owner_changed.set(true);
    let tools = crate::package_release_signer::fixture_measured_tools(
        input.journal_receipt.expected_tools.clone(),
    );
    assert!(crate::crate_release_journal::record_unprivileged_completion(input, tools).is_err());
    input.registry.owner_changed.set(false);
    input.registry.base.archive.borrow_mut().push(0);
    let tools = crate::package_release_signer::fixture_measured_tools(
        input.journal_receipt.expected_tools.clone(),
    );
    assert!(crate::crate_release_journal::record_unprivileged_completion(input, tools).is_err());
    assert_eq!(input.journal.write_calls.get(), writes);
    assert_eq!(input.github.writes.get(), 2);
}

pub(super) struct CreationWorkspaceGithub(pub(super) MixedGithub);
impl GitHubAdmissionRead for CreationWorkspaceGithub {
    fn get(&self, route: &str) -> Result<Value, OrchestratorError> {
        if route.ends_with("/actions/runs/12") {
            self.0.inner.0.get(route)
        } else {
            self.0.get(route)
        }
    }
    fn get_optional(&self, route: &str) -> Result<Option<Value>, OrchestratorError> {
        self.0.get_optional(route)
    }
}
impl CrateReleaseRepairWrite for CreationWorkspaceGithub {
    fn create_tag(&self, repo: &str, tag: &str, sha: &str) -> Result<(), OrchestratorError> {
        self.0.create_tag(repo, tag, sha)
    }
    fn create_release(
        &self,
        repo: &str,
        tag: &str,
        sha: &str,
        version: &str,
    ) -> Result<(), OrchestratorError> {
        self.0.create_release(repo, tag, sha, version)
    }
}
pub(super) struct CreationWorkspaceRegistry {
    pub(super) inner: MixedRegistry,
    pub(super) owner_changed: Cell<bool>,
}
impl CrateRegistryRead for CreationWorkspaceRegistry {
    fn get(&self, path: &str) -> Result<CrateRegistryResponse, OrchestratorError> {
        for (index, suffix) in ["a", "b"].iter().enumerate() {
            let name = format!("velnor_fixture_order_20261104_{suffix}");
            let root = format!("/api/v1/crates/{name}");
            let visible = if index == 0 {
                self.inner.first.visible.get()
            } else {
                self.inner.second.borrow().is_some()
            };
            if path == root || path.starts_with(&format!("{root}/")) {
                if !visible {
                    return Ok(CrateRegistryResponse {
                        status: 404,
                        body: vec![],
                    });
                }
                if path == root {
                    let id = if index == 0 { 2 } else { 4 };
                    return Ok(CrateRegistryResponse { status: 200, body: json!({"crate":{"id":name,"name":name,"versions":[id]},"versions":[{"id":id,"crate":name,"num":"0.1.1","yanked":false}]}).to_string().into_bytes() });
                }
                if index == 0 && self.owner_changed.get() && path == format!("{root}/owners") {
                    return Ok(CrateRegistryResponse {
                        status: 200,
                        body: br#"{"users":[{"id":370533,"login":"owner","kind":"user"}]}"#
                            .to_vec(),
                    });
                }
            }
        }
        self.inner.get(path)
    }
}
