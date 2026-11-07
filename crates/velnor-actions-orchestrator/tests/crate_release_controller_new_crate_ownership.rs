// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    Cell, CrateRegistryRead, CrateRegistryResponse, Github, JournalRegistryFake, NewRegistry,
    OrchestratorError, fixture_archive, fixture_journal_plan, fixture_no_baseline, make_fixture,
    prepare_crate_release, test_git, velnor_actions_orchestrator,
};

pub(super) fn journal_existing_owners(
    value: &crate::crate_release_journal::JournalOwnership,
) -> Vec<crate::crate_release_journal::JournalOwner> {
    let crate::crate_release_journal::JournalOwnership::Existing { owners } = value else {
        panic!("expected existing owners")
    };
    owners.clone()
}

#[test]
pub(super) fn explicitly_approved_new_crate_admission_records_unknown_ownership()
-> Result<(), Box<dyn std::error::Error>> {
    struct Absent;
    impl CrateRegistryRead for Absent {
        fn get(&self, path: &str) -> Result<CrateRegistryResponse, OrchestratorError> {
            if path.starts_with("/api/v1/crates/demo") {
                Ok(CrateRegistryResponse {
                    status: 404,
                    body: Vec::new(),
                })
            } else {
                Err(OrchestratorError::Contract {
                    problem: "unexpected route".into(),
                })
            }
        }
    }
    let temp = tempfile::tempdir()?;
    let (mut intent, request, github, _) = make_fixture(temp.path())?;
    intent.authentication = velnor_actions_contract::config::ReleaseAuthentication::BootstrapToken;
    intent.recovery = None;
    intent.new_crates = Some(velnor_actions_contract::config::NewCrateRelease {
        source_sha: request.source_sha.clone(),
        packages: std::collections::BTreeMap::from([("demo".into(), "0.1.1".into())]),
    });
    github.approved.set(true);
    let receipt = prepare_crate_release(&intent, &request, &github, &Absent, temp.path())?;
    assert_eq!(
        receipt.packages[0].owners,
        crate::crate_release_registry::CrateRegistryOwnership::NewCrate
    );
    assert_eq!(github.writes.get(), 0);
    let mut no_approval = intent.clone();
    no_approval.new_crates = None;
    assert!(prepare_crate_release(&no_approval, &request, &github, &Absent, temp.path(),).is_err());
    let mut non_dispatch = request.clone();
    non_dispatch.event = "push".into();
    assert!(prepare_crate_release(&intent, &non_dispatch, &github, &Absent, temp.path(),).is_err());
    let mut wrong_version = intent.clone();
    wrong_version
        .new_crates
        .as_mut()
        .ok_or("creation")?
        .packages
        .insert("demo".into(), "0.1.2".into());
    assert!(
        prepare_crate_release(&wrong_version, &request, &github, &Absent, temp.path(),).is_err()
    );
    assert_eq!(github.writes.get(), 0);
    Ok(())
}

#[test]
pub(super) fn new_crate_upload_observation_requires_durable_intent_and_real_reviewer_owner()
-> Result<(), Box<dyn std::error::Error>> {
    use velnor_actions_orchestrator::crate_release_controller::{
        CrateReleaseObservation, record_verified_mutation,
    };
    use velnor_actions_orchestrator::crate_release_journal::{
        JournalAuthentication, JournalMutationKind, JournalOwnership,
        fixture_storage::seed_started_record, record_journal_intent,
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
    let registry = NewRegistry {
        base,
        visible: Cell::new(false),
        owner_changed: Cell::new(false),
    };
    let admission = prepare_crate_release(&intent, &request, &github, &registry, root)?;
    assert_eq!(
        admission.packages[0].owners,
        crate::crate_release_registry::CrateRegistryOwnership::NewCrate
    );
    let (identity, mut plan) = fixture_journal_plan(root, &intent, &request, &admission)?;
    plan.authentication = JournalAuthentication::BootstrapToken;
    plan.packages[0].owners = JournalOwnership::NewCrate;
    let journal = JournalRegistryFake::default();
    seed_started_record(&journal, &identity, &plan)?;
    let intended = new_crate_upload_intention();
    let obs = CrateReleaseObservation {
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
        sequence: 2,
    };
    assert!(record_verified_mutation(&obs).is_err());
    record_journal_intent(&journal, &identity, &plan, &intended)?;
    *registry.base.archive.borrow_mut() = fixture_archive(root)?;
    registry.base.absent.set(false);
    registry.visible.set(true);
    github.owner_match.set(false);
    let before = journal.write_calls.get();
    assert!(record_verified_mutation(&obs).is_err());
    assert_eq!(journal.write_calls.get(), before);
    github.owner_match.set(true);
    let metadata = CrateReleaseObservation { sequence: 3, ..obs };
    let before_metadata = github.writes.get();
    assert!(
        crate::crate_release_controller::repair_verified_metadata(
            &metadata,
            "demo",
            JournalMutationKind::CreateTag
        )
        .is_err()
    );
    assert_eq!(github.writes.get(), before_metadata);
    let upload_obs = CrateReleaseObservation {
        sequence: 2,
        ..metadata
    };
    record_verified_mutation(&upload_obs)?;
    verify_recorded_new_crate_owner(&journal, &identity)?;
    verify_new_crate_metadata(&upload_obs, &github, &registry, before_metadata)?;
    Ok(())
}

pub(super) fn new_crate_upload_intention() -> crate::crate_release_journal::JournalIntendedMutation
{
    use crate::crate_release_journal::{JournalIntendedMutation, JournalMutationKind};
    JournalIntendedMutation {
        sequence: 1,
        package: "demo".into(),
        version: "0.1.1".into(),
        kind: JournalMutationKind::Upload,
        semver_evidence: Some(fixture_no_baseline()),
    }
}

pub(super) fn verify_recorded_new_crate_owner(
    journal: &JournalRegistryFake,
    identity: &crate::crate_release_journal::JournalIdentity,
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::crate_release_journal::{
        JournalMutationPhase, JournalMutationResult, inspect_journal_mutation,
    };
    let record = inspect_journal_mutation(journal, identity, 2)?.ok_or("missing")?;
    let JournalMutationPhase::Observed {
        result: JournalMutationResult::Upload { owners, .. },
    } = record.phase
    else {
        return Err("expected upload".into());
    };
    assert_eq!(owners.len(), 1);
    assert_eq!(owners[0].login, "owner");
    Ok(())
}

pub(super) fn verify_new_crate_metadata(
    upload_obs: &crate::crate_release_controller::CrateReleaseObservation<
        '_,
        JournalRegistryFake,
        Github,
        NewRegistry,
    >,
    github: &Github,
    registry: &NewRegistry,
    before_metadata: u32,
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::crate_release_controller::CrateReleaseObservation;
    use crate::crate_release_journal::JournalMutationKind;
    let metadata = CrateReleaseObservation {
        sequence: 3,
        ..*upload_obs
    };
    github.owner_match.set(false);
    assert!(
        crate::crate_release_controller::repair_verified_metadata(
            &metadata,
            "demo",
            JournalMutationKind::CreateTag
        )
        .is_err()
    );
    assert_eq!(github.writes.get(), before_metadata);
    github.owner_match.set(true);
    registry.owner_changed.set(true);
    assert!(
        crate::crate_release_controller::repair_verified_metadata(
            &metadata,
            "demo",
            JournalMutationKind::CreateTag
        )
        .is_err()
    );
    assert_eq!(github.writes.get(), before_metadata);
    registry.owner_changed.set(false);
    github.release_variant.set(4);
    crate::crate_release_controller::repair_verified_metadata(
        &metadata,
        "demo",
        JournalMutationKind::CreateTag,
    )?;
    assert_eq!(github.writes.get(), before_metadata + 1);
    let release_metadata = CrateReleaseObservation {
        sequence: 5,
        ..metadata
    };
    crate::crate_release_controller::repair_verified_metadata(
        &release_metadata,
        "demo",
        JournalMutationKind::CreateRelease,
    )?;
    assert_eq!(github.writes.get(), before_metadata + 2);
    let completion = CrateReleaseObservation {
        sequence: 7,
        ..release_metadata
    };
    let candidate = crate::crate_release_controller::derive_reconciled_candidate(&completion)?;
    assert_eq!(candidate.packages.len(), 1);
    Ok(())
}

#[test]
pub(super) fn new_crate_reconciliation_accepts_verified_first_owner()
-> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let (mut intent, request, github, base) = make_fixture(temp.path())?;
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
    let receipt = prepare_crate_release(&intent, &request, &github, &registry, temp.path())?;
    *registry.base.archive.borrow_mut() = fixture_archive(temp.path())?;
    registry.base.absent.set(false);
    registry.visible.set(true);
    github.tag_exists.set(true);
    let check = || {
        velnor_actions_orchestrator::crate_release_controller::validate_reconciliation_receipt(
            &receipt,
            &intent,
            &request,
            &github,
            &registry,
            crate::CrateSource::new(temp.path(), test_git(), &crate::UnprivilegedSourceExecution),
        )
    };
    assert!(check()?);
    let completed = velnor_actions_orchestrator::crate_release_controller::reconcile_crate_release(
        &receipt,
        &intent,
        &request,
        &github,
        &registry,
        crate::CrateSource::new(temp.path(), test_git(), &crate::UnprivilegedSourceExecution),
    )?;
    assert_eq!(completed[0].tag_commit, request.source_sha);
    github.owner_match.set(false);
    assert!(check().is_err());
    assert!(
        velnor_actions_orchestrator::crate_release_controller::reconcile_crate_release(
            &receipt,
            &intent,
            &request,
            &github,
            &registry,
            crate::CrateSource::new(temp.path(), test_git(), &crate::UnprivilegedSourceExecution)
        )
        .is_err()
    );
    Ok(())
}

#[test]
pub(super) fn missing_semver_baseline_requires_reviewed_new_crate()
-> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    let (mut intent, request, github, base) = make_fixture(root)?;
    github.approved.set(true);
    let existing = prepare_crate_release(&intent, &request, &github, &base, root)?;
    let registry = NewRegistry {
        base,
        visible: Cell::new(false),
        owner_changed: Cell::new(false),
    };
    let check = |receipt: &crate::crate_release_controller::CrateReleaseAdmissionReceipt| {
        crate::release::crate_release_semver::verify_one(
            root,
            receipt,
            &registry,
            &receipt.packages[0],
            root,
            test_git(),
            &crate::UnprivilegedSourceExecution,
        )
    };
    assert!(check(&existing).is_err());
    intent.authentication = velnor_actions_contract::config::ReleaseAuthentication::BootstrapToken;
    intent.recovery = None;
    intent.new_crates = Some(velnor_actions_contract::config::NewCrateRelease {
        source_sha: request.source_sha.clone(),
        packages: std::collections::BTreeMap::from([("demo".into(), "0.1.1".into())]),
    });
    let new = prepare_crate_release(&intent, &request, &github, &registry, root)?;
    assert!(matches!(
        check(&new)?,
        crate::release::crate_release_semver::SemverEvidence::NoBaseline { .. }
    ));
    let mut wrong = new.clone();
    wrong
        .intent
        .new_crates
        .as_mut()
        .ok_or("creation")?
        .packages
        .insert("demo".into(), "0.1.2".into());
    assert!(check(&wrong).is_err());
    let mut wrong = new.clone();
    wrong
        .intent
        .new_crates
        .as_mut()
        .ok_or("creation")?
        .source_sha = "f".repeat(40);
    assert!(check(&wrong).is_err());
    let mut wrong = new.clone();
    wrong.intent.new_crates = None;
    assert!(check(&wrong).is_err());
    let mut wrong = new.clone();
    wrong.intent.authentication =
        velnor_actions_contract::config::ReleaseAuthentication::TrustedPublishing;
    assert!(check(&wrong).is_err());
    let mut wrong = new.clone();
    wrong.github.request.event = "push".into();
    assert!(check(&wrong).is_err());
    let mut wrong = new.clone();
    wrong.packages[0].action = crate::crate_release_controller::CrateReleaseAction::RepairTag {
        registry: crate::crate_release_registry::CrateRegistryReconciliation {
            checksum: "a".repeat(64),
            downloaded_checksum: "a".repeat(64),
            owners: vec![],
            features: std::collections::BTreeMap::new(),
        },
    };
    assert!(check(&wrong).is_err());
    Ok(())
}
