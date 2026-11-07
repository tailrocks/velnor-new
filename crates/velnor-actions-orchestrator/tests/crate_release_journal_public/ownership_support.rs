// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

pub(super) fn journal_existing_owners(
    value: &crate::crate_release_journal::JournalOwnership,
) -> Vec<crate::crate_release_journal::JournalOwner> {
    let crate::crate_release_journal::JournalOwnership::Existing { owners } = value else {
        panic!("expected existing owners")
    };
    owners.clone()
}

pub(super) fn verify_new_crate_prior_intention(
    plan: &JournalReceipt,
    root: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::crate_release_journal::{
        JournalIntendedMutation, JournalMutationKind, JournalOwnership,
        fixture_storage::seed_started_record, record_journal_intent,
    };
    use crate::release::crate_release_semver::SemverEvidence;
    let journal = Registry::default();
    let mut prior = plan.clone();
    prior.authentication = JournalAuthentication::BootstrapToken;
    prior.packages[0].owners = JournalOwnership::NewCrate;
    seed_started_record(&journal, &prior.identity, &prior)?;
    let mut current = prior.clone();
    current.identity.run_id += 1;
    current.workflow_run_url = "https://github.com/example/demo-crate/actions/runs/43".into();
    current.predecessor = Some(prior.identity.clone());
    current.packages[0].owners = plan.packages[0].owners.clone();
    current.semver_evidence.insert(
        "demo-crate".into(),
        SemverEvidence::PriorNoBaselinePending {
            checker_sha256: "bc9eb06ce04e97d6160c875ffdee1f6e56793e423dea025f344826be03e66829"
                .into(),
            target_checksum: "d".repeat(64),
        },
    );
    assert!(inspect_prior_history(&journal, &prior.identity, &current, root, test_git()).is_err());
    record_journal_intent(
        &journal,
        &prior.identity,
        &prior,
        &JournalIntendedMutation {
            sequence: 1,
            package: "demo-crate".into(),
            version: "0.1.1".into(),
            kind: JournalMutationKind::Upload,
            semver_evidence: Some(SemverEvidence::NoBaseline {
                checker_sha256: "bc9eb06ce04e97d6160c875ffdee1f6e56793e423dea025f344826be03e66829"
                    .into(),
            }),
        },
    )?;
    assert!(inspect_prior_history(&journal, &prior.identity, &current, root, test_git()).is_ok());
    reject_moved_history_ref(&journal, &prior, &current, root);
    reject_forged_new_crate_prior(plan, &prior, &current, root)?;
    let actual_owners = journal_existing_owners(&current.packages[0].owners);
    crate::crate_release_journal::record_journal_mutation(
        &journal,
        &prior.identity,
        &prior,
        &crate::crate_release_journal::JournalMutation {
            sequence: 2,
            package: "demo-crate".into(),
            version: "0.1.1".into(),
            kind: JournalMutationKind::Upload,
            semver_evidence: None,
            phase: crate::crate_release_journal::JournalMutationPhase::Observed {
                result: crate::crate_release_journal::JournalMutationResult::Upload {
                    registry_checksum: "d".repeat(64),
                    downloaded_checksum: "d".repeat(64),
                    owners: actual_owners,
                    features: BTreeMap::new(),
                    files: vec![JournalFile {
                        path: "src/lib.rs".into(),
                        sha256: "a".repeat(64),
                        kind: JournalFileKind::Regular { executable: false },
                    }],
                },
            },
        },
    )?;
    assert!(inspect_prior_history(&journal, &prior.identity, &current, root, test_git()).is_ok());
    reject_changed_new_crate_result(&journal, &prior, &current, root)?;
    reject_pending_upload(&journal, &current)?;
    Ok(())
}

fn reject_pending_upload(
    journal: &Registry,
    current: &JournalReceipt,
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::crate_release_journal::{
        JournalIntendedMutation, JournalMutationKind, fixture_storage::seed_started_record,
        record_journal_intent,
    };
    use crate::release::crate_release_semver::SemverEvidence;
    seed_started_record(journal, &current.identity, current)?;
    let counts = (
        journal.refs.borrow().len(),
        journal.tags.borrow().len(),
        journal.blobs.borrow().len(),
    );
    let forged = JournalIntendedMutation {
        sequence: 1,
        package: "demo-crate".into(),
        version: "0.1.1".into(),
        kind: JournalMutationKind::Upload,
        semver_evidence: Some(SemverEvidence::NoBaseline {
            checker_sha256: "bc9eb06ce04e97d6160c875ffdee1f6e56793e423dea025f344826be03e66829"
                .into(),
        }),
    };
    assert!(
        record_journal_intent(journal, &current.identity, current, &forged).is_err(),
        "pending historical upload must not authorize another upload"
    );
    assert_eq!(
        counts,
        (
            journal.refs.borrow().len(),
            journal.tags.borrow().len(),
            journal.blobs.borrow().len()
        )
    );
    Ok(())
}

pub(super) fn reject_moved_history_ref(
    journal: &Registry,
    prior: &JournalReceipt,
    current: &JournalReceipt,
    root: &std::path::Path,
) {
    let original_refs = journal.refs.borrow().clone();
    let object_counts = (journal.tags.borrow().len(), journal.blobs.borrow().len());
    journal.move_on_second_list.set(1);
    assert!(
        inspect_prior_history(journal, &prior.identity, current, root, test_git()).is_err(),
        "moving a journal ref between history observations must be rejected"
    );
    assert_eq!(
        (journal.tags.borrow().len(), journal.blobs.borrow().len()),
        object_counts
    );
    *journal.refs.borrow_mut() = original_refs;
}

pub(super) fn reject_changed_new_crate_result(
    journal: &Registry,
    prior: &JournalReceipt,
    current: &JournalReceipt,
    root: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::crate_release_journal::JournalOwnership;
    use crate::release::crate_release_semver::SemverEvidence;
    let mut wrong_checksum = current.clone();
    wrong_checksum.semver_evidence.insert(
        "demo-crate".into(),
        SemverEvidence::PriorNoBaselinePending {
            checker_sha256: "bc9eb06ce04e97d6160c875ffdee1f6e56793e423dea025f344826be03e66829"
                .into(),
            target_checksum: "e".repeat(64),
        },
    );
    assert!(
        inspect_prior_history(journal, &prior.identity, &wrong_checksum, root, test_git()).is_err()
    );
    let mut wrong_owner = current.clone();
    let JournalOwnership::Existing { owners } = &mut wrong_owner.packages[0].owners else {
        return Err("owner".into());
    };
    owners[0].id += 1;
    assert!(
        inspect_prior_history(journal, &prior.identity, &wrong_owner, root, test_git()).is_err()
    );
    let mut bad = current.clone();
    bad.identity.source_sha = "f".repeat(40);
    assert!(inspect_prior_history(journal, &prior.identity, &bad, root, test_git()).is_err());
    Ok(())
}

pub(super) fn reject_forged_new_crate_prior(
    plan: &JournalReceipt,
    prior: &JournalReceipt,
    current: &JournalReceipt,
    root: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::crate_release_journal::{
        JournalIntendedMutation, JournalMutationKind, fixture_storage::seed_started_record,
        record_journal_intent,
    };
    use crate::release::crate_release_semver::SemverEvidence;
    let forged_journal = Registry::default();
    let mut existing_prior = prior.clone();
    existing_prior.packages[0].owners = plan.packages[0].owners.clone();
    seed_started_record(&forged_journal, &existing_prior.identity, &existing_prior)?;
    record_journal_intent(
        &forged_journal,
        &existing_prior.identity,
        &existing_prior,
        &JournalIntendedMutation {
            sequence: 1,
            package: "demo-crate".into(),
            version: "0.1.1".into(),
            kind: JournalMutationKind::Upload,
            semver_evidence: Some(SemverEvidence::NoBaseline {
                checker_sha256: "bc9eb06ce04e97d6160c875ffdee1f6e56793e423dea025f344826be03e66829"
                    .into(),
            }),
        },
    )?;
    assert!(
        inspect_prior_history(
            &forged_journal,
            &existing_prior.identity,
            current,
            root,
            test_git()
        )
        .is_err()
    );
    Ok(())
}
