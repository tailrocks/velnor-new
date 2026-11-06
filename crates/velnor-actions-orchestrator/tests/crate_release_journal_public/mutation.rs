// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn mutation_records_require_a_contiguous_immutable_chain() -> Result<(), Box<dyn std::error::Error>>
{
    use velnor_actions_orchestrator::crate_release_journal::{
        JournalMutation, JournalMutationKind, JournalMutationPhase, JournalMutationResult,
        inspect_journal_mutation, record_journal_mutation,
    };
    let remote = Registry::default();
    let plan = receipt("0.1.1");
    let mut item = JournalMutation {
        semver_evidence: Some(crate::crate_release_semver::SemverEvidence::NoBaseline {
            checker_sha256: velnor_actions_mise::catalog::qualified_cargo_semver_checks(
                "ubuntu-26.04",
            )
            .expect("qualified checker")
            .executable_sha256
            .into(),
        }),
        sequence: 1,
        package: "demo-crate".into(),
        version: "0.1.1".into(),
        kind: JournalMutationKind::Upload,
        phase: JournalMutationPhase::Intended,
    };
    assert!(record_journal_mutation(&remote, &identity(), &plan, &item).is_err());
    record_journal(&remote, &identity(), JournalStage::Started, &plan, None)?;
    let mut wrong = item.clone();
    wrong.package = "other".into();
    assert!(record_journal_mutation(&remote, &identity(), &plan, &wrong).is_err());
    record_journal_mutation(&remote, &identity(), &plan, &item)?;
    assert_eq!(
        inspect_journal_mutation(&remote, &identity(), 1)?,
        Some(item.clone())
    );
    assert!(record_journal_mutation(&remote, &identity(), &plan, &item).is_ok());
    item.sequence = 3;
    assert!(record_journal_mutation(&remote, &identity(), &plan, &item).is_err());
    item.sequence = 2;
    item.semver_evidence = None;
    item.phase = JournalMutationPhase::Observed {
        result: JournalMutationResult::Tag {
            name: "demo-crate-v0.1.1".into(),
            target_sha: "b".repeat(40),
        },
    };
    assert!(record_journal_mutation(&remote, &identity(), &plan, &item).is_err());
    item.semver_evidence = None;
    item.phase = JournalMutationPhase::Observed {
        result: JournalMutationResult::Upload {
            registry_checksum: "d".repeat(64),
            downloaded_checksum: "d".repeat(64),
            owners: journal_existing_owners(&plan.packages[0].owners),
            features: BTreeMap::new(),
            files: completion("0.1.1").packages[0].files.clone(),
        },
    };
    record_journal_mutation(&remote, &identity(), &plan, &item)?;
    assert_eq!(
        inspect_journal_mutation(&remote, &identity(), 2)?,
        Some(item)
    );
    Ok(())
}

#[test]
fn completion_rejects_a_pending_mutation() -> Result<(), Box<dyn std::error::Error>> {
    use velnor_actions_orchestrator::crate_release_journal::{
        JournalMutation, JournalMutationKind, JournalMutationPhase, JournalMutationResult,
        record_journal_mutation,
    };
    let remote = Registry::default();
    let plan = receipt("0.1.1");
    record_journal(&remote, &identity(), JournalStage::Started, &plan, None)?;
    let mut mutation = JournalMutation {
        semver_evidence: Some(crate::crate_release_semver::SemverEvidence::NoBaseline {
            checker_sha256: velnor_actions_mise::catalog::qualified_cargo_semver_checks(
                "ubuntu-26.04",
            )
            .expect("qualified checker")
            .executable_sha256
            .into(),
        }),
        sequence: 1,
        package: "demo-crate".into(),
        version: "0.1.1".into(),
        kind: JournalMutationKind::Upload,
        phase: JournalMutationPhase::Intended,
    };
    record_journal_mutation(&remote, &identity(), &plan, &mutation)?;
    assert!(super::super::inspect_current_history(&remote, &identity(), &plan).is_err());
    assert!(
        record_journal(
            &remote,
            &identity(),
            JournalStage::Completed,
            &plan,
            Some(&completion("0.1.1"))
        )
        .is_err()
    );
    assert_eq!(remote.refs.borrow().len(), 2);
    mutation.sequence = 2;
    mutation.semver_evidence = None;
    mutation.phase = JournalMutationPhase::Observed {
        result: JournalMutationResult::Upload {
            registry_checksum: "d".repeat(64),
            downloaded_checksum: "d".repeat(64),
            owners: journal_existing_owners(&plan.packages[0].owners),
            features: BTreeMap::new(),
            files: completion("0.1.1").packages[0].files.clone(),
        },
    };
    record_journal_mutation(&remote, &identity(), &plan, &mutation)?;
    assert_eq!(
        super::super::inspect_current_history(&remote, &identity(), &plan)?.len(),
        2
    );
    record_journal(
        &remote,
        &identity(),
        JournalStage::Completed,
        &plan,
        Some(&completion("0.1.1")),
    )?;
    mutation.sequence = 3;
    mutation.phase = JournalMutationPhase::Intended;
    assert!(record_journal_mutation(&remote, &identity(), &plan, &mutation).is_err());
    Ok(())
}

#[test]
fn mutation_record_retry_reads_back_exact_ambiguous_write() -> Result<(), Box<dyn std::error::Error>>
{
    use velnor_actions_orchestrator::crate_release_journal::{
        JournalMutation, JournalMutationKind, JournalMutationPhase, record_journal_mutation,
    };
    let mut remote = Registry::default();
    let plan = receipt("0.1.1");
    record_journal(&remote, &identity(), JournalStage::Started, &plan, None)?;
    let mutation = JournalMutation {
        semver_evidence: Some(crate::crate_release_semver::SemverEvidence::NoBaseline {
            checker_sha256: velnor_actions_mise::catalog::qualified_cargo_semver_checks(
                "ubuntu-26.04",
            )
            .expect("qualified checker")
            .executable_sha256
            .into(),
        }),
        sequence: 1,
        package: "demo-crate".into(),
        version: "0.1.1".into(),
        kind: JournalMutationKind::Upload,
        phase: JournalMutationPhase::Intended,
    };
    remote.fail_ref_before_write = Some(());
    assert!(record_journal_mutation(&remote, &identity(), &plan, &mutation).is_err());
    assert_eq!(remote.refs.borrow().len(), 1);
    remote.fail_ref_before_write = None;
    remote.fail_ref_after_write = true;
    assert!(record_journal_mutation(&remote, &identity(), &plan, &mutation).is_err());
    assert_eq!(remote.refs.borrow().len(), 2);
    let reference = record_journal_mutation(&remote, &identity(), &plan, &mutation)?;
    assert!(reference.ends_with("/operation-1"));
    assert_eq!(remote.refs.borrow().len(), 2);
    let mut conflicting = mutation.clone();
    conflicting.kind = JournalMutationKind::CreateTag;
    assert!(record_journal_mutation(&remote, &identity(), &plan, &conflicting).is_err());
    Ok(())
}

#[test]
fn mutation_results_match_package_operation_and_verified_shape()
-> Result<(), Box<dyn std::error::Error>> {
    use velnor_actions_orchestrator::crate_release_journal::{
        JournalMutation, JournalMutationKind as Kind, JournalMutationPhase as Phase,
        JournalMutationResult as ResultKind, record_journal_mutation,
    };
    let remote = Registry::default();
    let plan = receipt("0.1.1");
    record_journal(&remote, &identity(), JournalStage::Started, &plan, None)?;
    let mut item = JournalMutation {
        semver_evidence: Some(crate::crate_release_semver::SemverEvidence::NoBaseline {
            checker_sha256: velnor_actions_mise::catalog::qualified_cargo_semver_checks(
                "ubuntu-26.04",
            )
            .expect("qualified checker")
            .executable_sha256
            .into(),
        }),
        sequence: 1,
        package: "demo-crate".into(),
        version: "0.1.1".into(),
        kind: Kind::Upload,
        phase: Phase::Intended,
    };
    record_journal_mutation(&remote, &identity(), &plan, &item)?;
    item.sequence = 2;
    let upload = ResultKind::Upload {
        registry_checksum: "d".repeat(64),
        downloaded_checksum: "d".repeat(64),
        owners: journal_existing_owners(&plan.packages[0].owners),
        features: BTreeMap::new(),
        files: completion("0.1.1").packages[0].files.clone(),
    };
    let mut wrong = upload.clone();
    if let ResultKind::Upload {
        downloaded_checksum,
        ..
    } = &mut wrong
    {
        *downloaded_checksum = "f".repeat(64);
    }
    item.semver_evidence = None;
    item.phase = Phase::Observed { result: wrong };
    assert!(record_journal_mutation(&remote, &identity(), &plan, &item).is_err());
    let mut wrong = upload.clone();
    if let ResultKind::Upload { owners, .. } = &mut wrong {
        owners.clear();
    }
    item.semver_evidence = None;
    item.phase = Phase::Observed { result: wrong };
    assert!(record_journal_mutation(&remote, &identity(), &plan, &item).is_err());
    item.semver_evidence = None;
    item.phase = Phase::Observed { result: upload };
    record_journal_mutation(&remote, &identity(), &plan, &item)?;
    item.sequence = 3;
    item.kind = Kind::CreateTag;
    item.phase = Phase::Intended;
    record_journal_mutation(&remote, &identity(), &plan, &item)?;
    item.sequence = 4;
    item.semver_evidence = None;
    item.phase = Phase::Observed {
        result: ResultKind::Tag {
            name: "demo-crate-v0.1.1".into(),
            target_sha: "c".repeat(40),
        },
    };
    assert!(record_journal_mutation(&remote, &identity(), &plan, &item).is_err());
    item.semver_evidence = None;
    item.phase = Phase::Observed {
        result: ResultKind::Tag {
            name: "demo-crate-v0.1.1".into(),
            target_sha: "b".repeat(40),
        },
    };
    record_journal_mutation(&remote, &identity(), &plan, &item)?;
    verify_release_result_shape(&remote, &plan, &mut item)?;
    Ok(())
}

#[test]
fn exact_start_retry_reads_existing_record_without_writes() -> Result<(), Box<dyn std::error::Error>>
{
    let remote = Registry::default();
    let plan = receipt("0.1.1");
    let reference = record_journal(&remote, &identity(), JournalStage::Started, &plan, None)?;
    let blobs = remote.blobs.borrow().len();
    let tags = remote.tags.borrow().len();
    assert_eq!(
        record_journal(&remote, &identity(), JournalStage::Started, &plan, None)?,
        reference
    );
    assert_eq!(remote.refs.borrow().len(), 1);
    assert_eq!(remote.blobs.borrow().len(), blobs);
    assert_eq!(remote.tags.borrow().len(), tags);
    let mut changed = plan.clone();
    changed.trusted_reviewer_id += 1;
    assert!(record_journal(&remote, &identity(), JournalStage::Started, &changed, None).is_err());
    assert_eq!(remote.refs.borrow().len(), 1);
    Ok(())
}
