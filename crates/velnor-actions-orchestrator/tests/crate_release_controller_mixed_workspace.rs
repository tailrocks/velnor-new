// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    Cell, FIXTURE_SPARSE_INDEX, HistoricalAuthority, JournalRegistryFake, MixedGithub,
    MixedRegistry, OrchestratorError, RefCell, SparseFixture, assert_two_link_recovery,
    commit_history_intent, fixture_archive, fixture_journal_plan, fixture_upload_intention, fs,
    initialize_history_repo, make_fixture, make_workspace_order_fixture, prepare_crate_release,
    test_git,
};

#[test]
pub(super) fn mixed_workspace_coordinator_repairs_predecessor_before_upload()
-> Result<(), Box<dyn std::error::Error>> {
    use crate::crate_release_controller::{
        CrateReleaseObservation, UploadAttempt, coordinate_release,
    };
    use crate::crate_release_journal::fixture_storage::seed_started_record;
    let (temp, intent, request, original_github, registry) = make_workspace_order_fixture()?;
    let root = temp.path();
    let original = prepare_crate_release(&intent, &request, &original_github, &registry, root)?;
    let archive_dir = tempfile::tempdir()?;
    let first_archive = crate::crate_release_controller::package_unprivileged(
        root,
        &intent,
        &original.packages[0].source,
        archive_dir.path(),
        &request.source_sha,
        false,
        test_git(),
    )?;
    *registry.archive.borrow_mut() = first_archive.clone();
    registry.visible.set(true);
    let github = MixedGithub {
        inner: original_github,
        tags: [Cell::new(false), Cell::new(false)],
        releases: [Cell::new(false), Cell::new(false)],
        writes: RefCell::new(Vec::new()),
    };
    let registry = MixedRegistry {
        first: registry,
        second: RefCell::new(None),
    };
    let admission = prepare_crate_release(&intent, &request, &github, &registry, root)?;
    let mode = crate::release::crate_release_runtime::release_mode(&admission);
    assert_eq!(mode, "mixed");
    let (identity, mut plan) = fixture_journal_plan(root, &intent, &request, &admission)?;
    set_workspace_journal_packages(&mut plan, &admission);
    let journal = JournalRegistryFake::default();
    seed_started_record(&journal, &identity, &plan)?;
    let server = SparseFixture::start("velnor_fixture_order_20261104_a", first_archive)?;
    FIXTURE_SPARSE_INDEX.with(|value| *value.borrow_mut() = Some(server.url.clone()));
    let waits = Cell::new(0);
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
    let result = coordinate_release(
        &input,
        |name| mixed_verified_semver(&admission, &github, &server, name),
        |permit| {
            assert_eq!(permit.package, "velnor_fixture_order_20261104_b");
            assert_eq!(&*github.writes.borrow(), &["tag-0", "release-0"]);
            let target = tempfile::tempdir().map_err(|_| OrchestratorError::Contract {
                problem: "fixture".into(),
            })?;
            let archive = crate::crate_release_controller::package_unprivileged(
                root,
                &intent,
                &admission.packages[1].source,
                target.path(),
                &request.source_sha,
                false,
                test_git(),
            )?;
            *registry.second.borrow_mut() = Some(archive);
            Ok(UploadAttempt::ExitedSuccess)
        },
        || propagate_sparse_index(&registry, &server, &waits),
    );
    FIXTURE_SPARSE_INDEX.with(|value| *value.borrow_mut() = None);
    assert_eq!(waits.get(), 1);
    check_mixed_workspace_result(&github, result?.packages.len());
    Ok(())
}

pub(super) fn mixed_verified_semver(
    admission: &crate::crate_release_controller::CrateReleaseAdmissionReceipt,
    github: &MixedGithub,
    server: &SparseFixture,
    name: &str,
) -> Result<crate::release::crate_release_semver::VerifiedSemver, OrchestratorError> {
    assert_eq!(name, "velnor_fixture_order_20261104_b");
    assert_eq!(&*github.writes.borrow(), &["tag-0", "release-0"]);
    assert!(
        server
            .index_visible
            .load(std::sync::atomic::Ordering::SeqCst)
    );
    crate::release::crate_release_semver::fixture_verified_semver(admission, name)
}

pub(super) fn propagate_sparse_index(
    registry: &MixedRegistry,
    server: &SparseFixture,
    waits: &Cell<u32>,
) {
    waits.set(waits.get() + 1);
    registry.first.index_visible.set(true);
    server
        .index_visible
        .store(true, std::sync::atomic::Ordering::SeqCst);
}

pub(super) fn check_mixed_workspace_result(github: &MixedGithub, packages: usize) {
    assert_eq!(packages, 2);
    assert_eq!(
        &*github.writes.borrow(),
        &["tag-0", "release-0", "tag-1", "release-1"]
    );
}

pub(super) fn set_workspace_journal_packages(
    plan: &mut crate::crate_release_journal::JournalReceipt,
    admission: &crate::crate_release_controller::CrateReleaseAdmissionReceipt,
) {
    use crate::crate_release_journal::{JournalOwner, JournalOwnerKind, JournalPackage};
    plan.semver_evidence = admission.semver_evidence.clone();
    plan.packages = admission
        .packages
        .iter()
        .map(|p| JournalPackage {
            name: p.source.name.clone(),
            version: p.source.version.clone(),
            manifest: p.source.manifest.clone(),
            tag: p.tag.clone(),
            owners: crate::crate_release_journal::JournalOwnership::Existing {
                owners: vec![JournalOwner {
                    id: 370_532,
                    login: "owner".into(),
                    kind: JournalOwnerKind::User,
                }],
            },
        })
        .collect();
}

#[test]
pub(super) fn serial_coordinator_recovers_pending_visible_tag_without_recreating()
-> Result<(), Box<dyn std::error::Error>> {
    use crate::crate_release_controller::{CrateReleaseObservation, coordinate_release};
    use crate::crate_release_journal::{
        JournalIntendedMutation, JournalMutationKind, fixture_storage::seed_started_record,
        record_journal_intent,
    };
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    let (intent, request, github, registry) = make_fixture(root)?;
    github.approved.set(true);
    github.release_variant.set(4);
    let admission = prepare_crate_release(&intent, &request, &github, &registry, root)?;
    let (identity, plan) = fixture_journal_plan(root, &intent, &request, &admission)?;
    let journal = JournalRegistryFake::default();
    seed_started_record(&journal, &identity, &plan)?;
    record_journal_intent(
        &journal,
        &identity,
        &plan,
        &JournalIntendedMutation {
            semver_evidence: None,
            sequence: 1,
            package: "demo".into(),
            version: "0.1.1".into(),
            kind: JournalMutationKind::CreateTag,
        },
    )?;
    *registry.archive.borrow_mut() = fixture_archive(root)?;
    registry.absent.set(false);
    github.tag_exists.set(true);
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
    let result = coordinate_release(
        &input,
        |name| crate::release::crate_release_semver::fixture_verified_semver(input.admission, name),
        |_| panic!("must not upload an existing version"),
        || {},
    )?;
    assert_eq!(result.history.len(), 4);
    assert_eq!(github.writes.get(), 1);
    Ok(())
}

#[test]
pub(super) fn admitted_start_binds_previous_workflow_commit_and_reobserves_prior_upload()
-> Result<(), Box<dyn std::error::Error>> {
    use crate::crate_release_journal::{
        fixture_storage::seed_started_record, record_journal_intent,
    };
    use crate::release::package_release_signer::{
        AdmittedStart, fixture_attested_generator, record_admitted_start,
    };
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    let workflow = tempfile::tempdir()?;
    initialize_history_repo(workflow.path())?;
    let (mut intent, request, github, registry) = make_fixture(root)?;
    github.approved.set(true);
    let mut previous_intent = intent.clone();
    let older_workflow_sha =
        commit_history_intent(workflow.path(), &previous_intent, "previous workflow")?;
    let original = prepare_crate_release(&intent, &request, &github, &registry, root)?;
    let (_, mut older) = fixture_journal_plan(root, &previous_intent, &request, &original)?;
    older.identity.run_id = 10;
    older.identity.workflow_sha = older_workflow_sha.clone();
    older.workflow_run_url = "https://github.com/example/demo/actions/runs/10".into();
    let journal = JournalRegistryFake::default();
    seed_started_record(&journal, &older.identity, &older)?;
    record_journal_intent(
        &journal,
        &older.identity,
        &older,
        &fixture_upload_intention(),
    )?;
    previous_intent.journal_predecessor =
        Some(velnor_actions_contract::config::CrateReleasePredecessor {
            run_id: 10,
            run_attempt: 1,
            workflow_sha: older_workflow_sha.clone(),
        });
    let previous_workflow_sha =
        commit_history_intent(workflow.path(), &previous_intent, "prior workflow")?;
    let (_, mut previous) = fixture_journal_plan(root, &previous_intent, &request, &original)?;
    previous.identity.run_id = 11;
    previous.identity.workflow_sha = previous_workflow_sha.clone();
    previous.workflow_run_url = "https://github.com/example/demo/actions/runs/11".into();
    previous.predecessor = Some(older.identity.clone());
    seed_started_record(&journal, &previous.identity, &previous)?;
    intent.journal_predecessor = Some(velnor_actions_contract::config::CrateReleasePredecessor {
        run_id: 11,
        run_attempt: 1,
        workflow_sha: previous_workflow_sha.clone(),
    });
    let admission = prepare_crate_release(&intent, &request, &github, &registry, root)?;
    let generator = fixture_attested_generator();
    let auth = HistoricalAuthority {
        base: &github,
        workflow: previous_workflow_sha.clone(),
        older: Some(older_workflow_sha),
        event: "workflow_dispatch",
        defect: Cell::new(0),
    };
    let input = AdmittedStart {
        source_execution: &crate::UnprivilegedSourceExecution,
        git: test_git(),
        journal: &journal,
        github: &auth,
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
    record_admitted_start(&input)?;
    assert_two_link_recovery(&input, &previous)?;
    Ok(())
}
