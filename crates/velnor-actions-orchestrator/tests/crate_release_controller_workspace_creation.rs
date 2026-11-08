// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    Cell, HistoricalAuthority, JournalRegistryFake, NewRegistry, WorkspaceGithub,
    WorkspaceRegistry, commit_history_intent, fixture_archive, fixture_journal_plan,
    fixture_no_baseline, initialize_history_repo, make_fixture, make_workspace_order_fixture,
    prepare_crate_release, set_workspace_journal_packages, test_git,
};

#[test]
pub(super) fn two_new_workspace_crates_require_each_own_recorded_upload()
-> Result<(), Box<dyn std::error::Error>> {
    use crate::crate_release_controller::CrateReleaseObservation;
    use crate::crate_release_journal::{
        JournalAuthentication, JournalOwnership, fixture_storage::seed_started_record,
        inspect_journal_mutation, record_journal_intent,
    };
    let (temp, mut intent, mut request, github, registry) = make_workspace_order_fixture()?;
    let root = temp.path();
    let original = prepare_crate_release(&intent, &request, &github, &registry, root)?;
    intent.authentication = velnor_actions_contract::config::ReleaseAuthentication::BootstrapToken;
    intent.new_crates = Some(velnor_actions_contract::config::NewCrateRelease {
        source_sha: request.source_sha.clone(),
        packages: original
            .packages
            .iter()
            .map(|package| (package.source.name.clone(), package.source.version.clone()))
            .collect(),
    });
    request.event = "workflow_dispatch".into();
    request.recovery_sha = Some(request.source_sha.clone());
    let mut admission = original.clone();
    admission.intent = intent.clone();
    admission.github.request = request.clone();
    admission.packages.iter_mut().for_each(|package| {
        package.owners = crate::crate_release_registry::CrateRegistryOwnership::NewCrate;
    });
    let current = created_workspace_admission(&admission, &original)?;
    let (identity, mut plan) = fixture_journal_plan(root, &intent, &request, &admission)?;
    plan.authentication = JournalAuthentication::BootstrapToken;
    set_workspace_journal_packages(&mut plan, &admission);
    for package in &mut plan.packages {
        package.owners = JournalOwnership::NewCrate;
    }
    let journal = JournalRegistryFake::default();
    seed_started_record(&journal, &identity, &plan)?;
    let first = &admission.packages[0].source;
    let second = &admission.packages[1].source;
    record_journal_intent(
        &journal,
        &identity,
        &plan,
        &fixture_creation_intention(1, &first.name, &first.version),
    )?;
    record_first_created_workspace_crate(&journal, &identity, &plan, first)?;
    record_journal_intent(
        &journal,
        &identity,
        &plan,
        &fixture_creation_intention(3, &second.name, &second.version),
    )?;
    let pending = inspect_journal_mutation(&journal, &identity, 3)?.ok_or("intent")?;
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
        sequence: 4,
    };
    super::super::observation::verify_observed_packages_with_upload_intent(
        &input,
        &current,
        Some(&pending),
    )?;
    reject_changed_workspace_owner(&input, &current, &pending)?;
    Ok(())
}

pub(super) fn reject_changed_workspace_owner(
    input: &crate::crate_release_controller::CrateReleaseObservation<
        '_,
        JournalRegistryFake,
        WorkspaceGithub,
        WorkspaceRegistry,
    >,
    current: &crate::crate_release_controller::CrateReleaseAdmissionReceipt,
    pending: &crate::crate_release_journal::JournalMutation,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut wrong = current.clone();
    let crate::crate_release_registry::CrateRegistryOwnership::Existing { owners } =
        &mut wrong.packages[0].owners
    else {
        return Err("owners".into());
    };
    owners[0].id += 1;
    assert!(
        super::super::observation::verify_observed_packages_with_upload_intent(
            input,
            &wrong,
            Some(pending)
        )
        .is_err()
    );
    Ok(())
}

pub(super) fn fixture_creation_intention(
    sequence: u32,
    name: &str,
    version: &str,
) -> crate::crate_release_journal::JournalIntendedMutation {
    crate::crate_release_journal::JournalIntendedMutation {
        archive_sha256: None,
        sequence,
        package: name.into(),
        version: version.into(),
        kind: crate::crate_release_journal::JournalMutationKind::Upload,
        semver_evidence: Some(fixture_no_baseline()),
    }
}

pub(super) fn created_workspace_admission(
    admission: &crate::crate_release_controller::CrateReleaseAdmissionReceipt,
    original: &crate::crate_release_controller::CrateReleaseAdmissionReceipt,
) -> Result<crate::crate_release_controller::CrateReleaseAdmissionReceipt, Box<dyn std::error::Error>>
{
    use crate::crate_release_controller::CrateReleaseAction;
    use crate::crate_release_registry::CrateRegistryReconciliation;
    let mut current = admission.clone();
    for (now, original) in current.packages.iter_mut().zip(&original.packages) {
        now.owners = original.owners.clone();
        let crate::crate_release_registry::CrateRegistryOwnership::Existing { owners } =
            &now.owners
        else {
            return Err("owners".into());
        };
        now.action = CrateReleaseAction::RepairTag {
            registry: CrateRegistryReconciliation {
                checksum: "d".repeat(64),
                downloaded_checksum: "d".repeat(64),
                owners: owners.clone(),
                features: std::collections::BTreeMap::new(),
            },
        };
    }
    Ok(current)
}

pub(super) fn record_first_created_workspace_crate(
    journal: &JournalRegistryFake,
    identity: &crate::crate_release_journal::JournalIdentity,
    plan: &crate::crate_release_journal::JournalReceipt,
    first: &crate::release::crate_release_source::CrateReleaseSourcePackage,
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::crate_release_journal::{
        JournalFile, JournalFileKind, JournalMutation, JournalMutationKind, JournalMutationPhase,
        JournalMutationResult, fixture_record_journal_mutation,
    };
    let recorded_owners = vec![crate::crate_release_journal::JournalOwner {
        id: 370_532,
        login: "owner".into(),
        kind: crate::crate_release_journal::JournalOwnerKind::User,
    }];
    fixture_record_journal_mutation(
        journal,
        identity,
        plan,
        &JournalMutation {
            archive_sha256: None,
            sequence: 2,
            package: first.name.clone(),
            version: first.version.clone(),
            kind: JournalMutationKind::Upload,
            semver_evidence: None,
            phase: JournalMutationPhase::Observed {
                result: JournalMutationResult::Upload {
                    registry_checksum: "d".repeat(64),
                    downloaded_checksum: "d".repeat(64),
                    owners: recorded_owners,
                    features: std::collections::BTreeMap::new(),
                    files: vec![JournalFile {
                        path: "src/lib.rs".into(),
                        sha256: "a".repeat(64),
                        kind: JournalFileKind::Regular { executable: false },
                    }],
                },
            },
        },
    )?;
    Ok(())
}

#[test]
pub(super) fn first_crate_recovery_rejects_unverified_custody_before_mutation()
-> Result<(), Box<dyn std::error::Error>> {
    use crate::crate_release_journal::{
        JournalAuthentication, JournalOwnership, fixture_storage::seed_started_record,
    };
    use crate::release::package_release_signer::{
        AdmittedStart, fixture_attested_generator, record_admitted_start,
    };
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    let workflow = tempfile::tempdir()?;
    initialize_history_repo(workflow.path())?;
    let (mut intent, request, github, base) = make_fixture(root)?;
    intent.authentication = velnor_actions_contract::config::ReleaseAuthentication::BootstrapToken;
    intent.recovery = None;
    intent.new_crates = Some(velnor_actions_contract::config::NewCrateRelease {
        source_sha: request.source_sha.clone(),
        packages: std::collections::BTreeMap::from([("demo".into(), "0.1.1".into())]),
    });
    github.approved.set(true);
    let registry = NewRegistry {
        base,
        visible: Cell::new(false),
        owner_changed: Cell::new(false),
    };
    let previous_workflow =
        commit_history_intent(workflow.path(), &intent, "prior creation workflow")?;
    let initial = prepare_crate_release(&intent, &request, &github, &registry, root)?;
    let (_, mut prior) = fixture_journal_plan(root, &intent, &request, &initial)?;
    prior.identity.run_id = 11;
    prior.identity.workflow_sha = previous_workflow.clone();
    prior.workflow_run_url = "https://github.com/example/demo/actions/runs/11".into();
    prior.authentication = JournalAuthentication::BootstrapToken;
    prior.packages[0].owners = JournalOwnership::NewCrate;
    let journal = JournalRegistryFake::default();
    seed_started_record(&journal, &prior.identity, &prior)?;
    *registry.base.archive.borrow_mut() = fixture_archive(root)?;
    registry.base.absent.set(false);
    registry.visible.set(true);
    intent.journal_predecessor = Some(velnor_actions_contract::config::CrateReleasePredecessor {
        run_id: 11,
        run_attempt: 1,
        workflow_sha: previous_workflow.clone(),
    });
    let auth = HistoricalAuthority {
        base: &github,
        workflow: previous_workflow,
        older: None,
        event: "workflow_dispatch",
        defect: Cell::new(0),
    };
    let admission = creation_recovery_preflight(
        root,
        prepare_crate_release(&intent, &request, &auth, &registry, root)?,
        &registry,
    )?;
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
        generator: &fixture_attested_generator(),
    };
    let before = journal.write_calls.get();
    let result = record_admitted_start(&input);
    assert!(
        matches!(result, Err(crate::OrchestratorError::Internal { problem }) if problem == "crate_release_history_custody_required")
    );
    assert_eq!(journal.write_calls.get(), before);
    assert_eq!(github.writes.get(), 0);
    Ok(())
}

fn creation_recovery_preflight(
    root: &std::path::Path,
    mut admission: crate::crate_release_controller::CrateReleaseAdmissionReceipt,
    registry: &NewRegistry,
) -> Result<crate::crate_release_controller::CrateReleaseAdmissionReceipt, Box<dyn std::error::Error>>
{
    let tools = tempfile::tempdir()?;
    admission.semver_evidence = crate::release::crate_release_semver::verify_preflight(
        root,
        &admission,
        registry,
        tools.path(),
        test_git(),
        &crate::UnprivilegedSourceExecution,
    )?;
    assert!(matches!(
        admission.semver_evidence.get("demo"),
        Some(crate::release::crate_release_semver::SemverEvidence::MetadataOnly { .. })
    ));
    Ok(admission)
}
