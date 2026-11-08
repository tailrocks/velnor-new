// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::mixed_workspace::{propagate_sparse_index, set_workspace_journal_packages};
use super::{
    Cell, FIXTURE_SPARSE_INDEX, JournalRegistryFake, MixedGithub, MixedRegistry, OrchestratorError,
    RefCell, SparseFixture, fixture_journal_plan, make_workspace_order_fixture,
    prepare_crate_release, test_git,
};

struct HistoricalMixedGithub {
    inner: MixedGithub,
    prior_sha: String,
    current_sha: String,
    forged_ancestry: Cell<bool>,
    release_reads: Cell<usize>,
    replace_release_at: Cell<usize>,
}

impl crate::crate_release_github::GitHubAdmissionRead for HistoricalMixedGithub {
    fn get(&self, route: &str) -> Result<serde_json::Value, OrchestratorError> {
        if route
            == format!(
                "repos/example/demo/compare/{}...{}",
                self.prior_sha, self.current_sha
            )
        {
            let base = if self.forged_ancestry.get() {
                &self.current_sha
            } else {
                &self.prior_sha
            };
            return Ok(
                serde_json::json!({"status":"ahead", "base_commit":{"sha":self.prior_sha}, "merge_base_commit":{"sha":base}}),
            );
        }
        crate::crate_release_github::GitHubAdmissionRead::get(&self.inner, route)
    }
    fn get_optional(&self, route: &str) -> Result<Option<serde_json::Value>, OrchestratorError> {
        if route.contains("git/ref/tags/velnor_fixture_order_20261104_a-v0.1.1") {
            return Ok(Some(
                serde_json::json!({"ref":"refs/tags/velnor_fixture_order_20261104_a-v0.1.1", "object":{"type":"commit","sha":self.prior_sha}}),
            ));
        }
        let mut result =
            crate::crate_release_github::GitHubAdmissionRead::get_optional(&self.inner, route)?;
        if route.contains("releases/tags/velnor_fixture_order_20261104_a-v0.1.1") {
            let count = self.release_reads.get() + 1;
            self.release_reads.set(count);
            if count >= self.replace_release_at.get()
                && let Some(value) = &mut result
            {
                value["id"] = serde_json::json!(999);
            }
        }
        Ok(result)
    }
}

impl crate::crate_release_controller::CrateReleaseRepairWrite for HistoricalMixedGithub {
    fn create_tag(&self, repository: &str, tag: &str, sha: &str) -> Result<(), OrchestratorError> {
        crate::crate_release_controller::CrateReleaseRepairWrite::create_tag(
            &self.inner,
            repository,
            tag,
            sha,
        )
    }
    fn create_release(
        &self,
        repository: &str,
        tag: &str,
        version: &str,
        sha: &str,
    ) -> Result<(), OrchestratorError> {
        crate::crate_release_controller::CrateReleaseRepairWrite::create_release(
            &self.inner,
            repository,
            tag,
            version,
            sha,
        )
    }
}

#[test]
fn historical_workspace_coordinator_rejects_forged_ancestry()
-> Result<(), Box<dyn std::error::Error>> {
    use crate::crate_release_controller::{CrateReleaseAction, CrateReleaseObservation};
    use crate::crate_release_journal::fixture_storage::seed_started_record;
    let fixture = historical_workspace_fixture()?;
    let HistoricalWorkspaceFixture {
        temp,
        intent,
        request,
        github,
        registry,
        archive,
    } = fixture;
    let root = temp.path();
    assert!(prepare_crate_release(&intent, &request, &github, &registry, root).is_err());
    assert!(github.inner.writes.borrow().is_empty());
    github.forged_ancestry.set(false);
    let admission = prepare_crate_release(&intent, &request, &github, &registry, root)?;
    assert!(matches!(
        admission.packages[0].action,
        CrateReleaseAction::Historical { .. }
    ));
    let mut changed_release = admission.clone();
    if let CrateReleaseAction::Historical { release, .. } = &mut changed_release.packages[0].action
    {
        release.id += 1;
    } else {
        return Err("historical action missing".into());
    }
    assert!(
        super::super::verify_observed_packages(&changed_release, &admission).is_err(),
        "historical release identity must not drift"
    );
    let (identity, mut plan) = fixture_journal_plan(root, &intent, &request, &admission)?;
    set_workspace_journal_packages(&mut plan, &admission);
    for (planned, admitted) in plan.packages.iter_mut().zip(&admission.packages) {
        planned.tag_target = Some(match &admitted.action {
            CrateReleaseAction::Historical { source_sha, .. } => source_sha.clone(),
            _ => request.source_sha.clone(),
        });
    }
    let journal = JournalRegistryFake::default();
    seed_started_record(&journal, &identity, &plan).map_err(|e| format!("seed: {e}"))?;
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
    run_historical_coordinator(&input, &changed_release, archive)?;
    Ok(())
}

struct HistoricalWorkspaceFixture {
    temp: tempfile::TempDir,
    intent: crate::crate_release_controller::CrateReleaseIntent,
    request: crate::crate_release_github::GitHubAdmissionRequest,
    github: HistoricalMixedGithub,
    registry: MixedRegistry,
    archive: Vec<u8>,
}

fn historical_workspace_fixture() -> Result<HistoricalWorkspaceFixture, Box<dyn std::error::Error>>
{
    let (temp, intent, mut request, mut base, registry) = make_workspace_order_fixture()?;
    let root = temp.path();
    let prior = request.source_sha.clone();
    let original = prepare_crate_release(&intent, &request, &base, &registry, root)?;
    let target = tempfile::tempdir()?;
    let archive = crate::crate_release_controller::package_unprivileged(
        root,
        &intent,
        &original.packages[0].source,
        target.path(),
        &prior,
        false,
        test_git(),
    )?;
    *registry.archive.borrow_mut() = archive.clone();
    registry.visible.set(true);
    std::fs::write(root.join("unrelated-history"), "reviewed unrelated change")?;
    super::git(root, &["add", "unrelated-history"])?;
    super::git(root, &["commit", "-q", "-s", "-m", "unrelated"])?;
    let current = super::git(root, &["rev-parse", "HEAD"])?;
    request.source_sha = current.clone();
    request.workflow_sha = current.clone();
    base.0.sha = current.clone();
    base.0.policy_sha = current.clone();
    let github = HistoricalMixedGithub {
        inner: MixedGithub {
            inner: base,
            tags: [Cell::new(true), Cell::new(false)],
            releases: [Cell::new(true), Cell::new(false)],
            writes: RefCell::new(Vec::new()),
        },
        prior_sha: prior,
        current_sha: current,
        forged_ancestry: Cell::new(true),
        release_reads: Cell::new(0),
        replace_release_at: Cell::new(usize::MAX),
    };
    let registry = MixedRegistry {
        first: registry,
        second: RefCell::new(None),
    };
    Ok(HistoricalWorkspaceFixture {
        temp,
        intent,
        request,
        github,
        registry,
        archive,
    })
}

fn run_historical_coordinator(
    input: &crate::crate_release_controller::CrateReleaseObservation<
        '_,
        JournalRegistryFake,
        HistoricalMixedGithub,
        MixedRegistry,
    >,
    changed_release: &crate::crate_release_controller::CrateReleaseAdmissionReceipt,
    archive: Vec<u8>,
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::crate_release_controller::{CrateReleaseObservation, coordinate_release};
    let server = SparseFixture::start("velnor_fixture_order_20261104_a", archive)?;
    FIXTURE_SPARSE_INDEX.with(|value| *value.borrow_mut() = Some(server.url.clone()));
    let waits = Cell::new(0);
    let result = coordinate_release(
        input,
        |name| crate::release::crate_release_semver::fixture_verified_semver(input.admission, name),
        |permit| {
            assert_eq!(permit.package, "velnor_fixture_order_20261104_b");
            let target = tempfile::tempdir().map_err(|_| OrchestratorError::Contract {
                problem: "fixture".into(),
            })?;
            let archive = crate::crate_release_controller::package_unprivileged(
                input.source,
                input.intent,
                &input.admission.packages[1].source,
                target.path(),
                &input.request.source_sha,
                false,
                test_git(),
            )?;
            *input.registry.second.borrow_mut() = Some(archive);
            Ok(crate::crate_release_controller::UploadAttempt::ExitedSuccess)
        },
        || propagate_sparse_index(input.registry, &server, &waits),
    );
    let result = result.map_err(|e| format!("coordinate: {e}"))?;
    assert_eq!(result.packages.len(), 2);
    assert_eq!(
        &*input.github.inner.writes.borrow(),
        &["tag-1", "release-1"]
    );
    reject_replaced_historical_release(input);
    let count = crate::crate_release_journal::JournalOperationRefs::list_operation_refs(
        input.journal,
        input.identity,
    )?
    .len();
    let after_mutations = CrateReleaseObservation {
        sequence: u32::try_from(count)? + 1,
        ..*input
    };
    assert!(
        super::super::observation::verify_observed_packages_after_recorded_upload(
            &after_mutations,
            changed_release
        )
        .is_err(),
        "historical identity must remain fixed after journal mutations"
    );
    crate::crate_release_journal::record_unprivileged_completion(
        input,
        crate::release::package_release_signer::fixture_measured_tools(
            input.journal_receipt.expected_tools.clone(),
        ),
    )
    .map_err(|e| format!("completion: {e}"))?;
    FIXTURE_SPARSE_INDEX.with(|value| *value.borrow_mut() = None);
    assert!(
        crate::crate_release_journal::inspect_journal(
            input.journal,
            input.identity,
            crate::crate_release_journal::JournalStage::Completed
        )?
        .is_some()
    );
    Ok(())
}

fn reject_replaced_historical_release(
    input: &crate::crate_release_controller::CrateReleaseObservation<
        '_,
        JournalRegistryFake,
        HistoricalMixedGithub,
        MixedRegistry,
    >,
) {
    input.github.release_reads.set(0);
    input.github.replace_release_at.set(3);
    let changed = crate::crate_release_controller::reconcile_crate_release(
        input.admission,
        input.intent,
        input.request,
        input.github,
        input.registry,
        crate::CrateSource::new(input.source, input.git, input.source_execution),
    );
    input.github.replace_release_at.set(usize::MAX);
    assert!(
        changed.is_err(),
        "replaced historical release must be rejected"
    );
}
