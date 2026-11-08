// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

pub(super) fn verify_release_result_shape(
    remote: &Registry,
    plan: &JournalReceipt,
    item: &mut velnor_actions_orchestrator::crate_release_journal::JournalMutation,
) -> Result<(), Box<dyn std::error::Error>> {
    use velnor_actions_orchestrator::crate_release_journal::{
        JournalMutationKind as Kind, JournalMutationPhase as Phase,
        JournalMutationResult as ResultKind, record_journal_mutation,
    };
    item.sequence = 5;
    item.kind = Kind::CreateRelease;
    item.phase = Phase::Intended;
    record_journal_mutation(remote, &identity(), plan, item)?;
    item.sequence = 6;
    item.semver_evidence = None;
    item.phase = Phase::Observed {
        result: ResultKind::Release {
            id: 0,
            tag: "demo-crate-v0.1.1".into(),
            url: "https://github.com/example/demo-crate/releases/tag/demo-crate-v0.1.1".into(),
        },
    };
    assert!(record_journal_mutation(remote, &identity(), plan, item).is_err());
    item.semver_evidence = None;
    item.phase = Phase::Observed {
        result: ResultKind::Release {
            id: 12,
            tag: "demo-crate-v0.1.1".into(),
            url: "https://evil.invalid/".into(),
        },
    };
    assert!(record_journal_mutation(remote, &identity(), plan, item).is_err());
    item.semver_evidence = None;
    item.phase = Phase::Observed {
        result: ResultKind::Release {
            id: 12,
            tag: "demo-crate-v0.1.1".into(),
            url: "https://github.com/example/demo-crate/releases/tag/demo-crate-v0.1.1".into(),
        },
    };
    record_journal_mutation(remote, &identity(), plan, item)?;
    Ok(())
}

pub(super) fn verify_chain_and_rerun(
    journal: &Registry,
    plan: &JournalReceipt,
    current: &JournalReceipt,
    root: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    verify_long_retry_history(plan, root)?;
    verify_history_source_change(plan, root)?;
    verify_history_snapshot(plan, root)?;
    let chained_journal = Registry::default();
    let mut older = plan.clone();
    older.identity.run_id -= 1;
    older.workflow_run_url = format!(
        "https://github.com/{}/actions/runs/{}",
        older.identity.repository, older.identity.run_id
    );
    seed_started_record(&chained_journal, &older.identity, &older)?;
    let mut chained = plan.clone();
    chained.predecessor = Some(older.identity.clone());
    seed_started_record(&chained_journal, &chained.identity, &chained)?;
    assert_eq!(
        super::super::inspect_prior_chain(&chained_journal, current, root, test_git())?.len(),
        2
    );
    let cyclic_journal = Registry::default();
    older.predecessor = Some(chained.identity.clone());
    seed_started_record(&cyclic_journal, &older.identity, &older)?;
    seed_started_record(&cyclic_journal, &chained.identity, &chained)?;
    assert!(super::super::inspect_prior_chain(&cyclic_journal, current, root, test_git()).is_err());
    let mut rerun = current.clone();
    rerun.identity.run_id = plan.identity.run_id;
    rerun.identity.run_attempt = plan.identity.run_attempt + 1;
    rerun.workflow_run_url = plan.workflow_run_url.clone();
    assert_eq!(
        inspect_prior_incomplete_evidence(journal, &plan.identity, &rerun, root, test_git())?
            .receipt,
        *plan
    );
    rerun.identity.run_attempt = plan.identity.run_attempt;
    assert!(
        inspect_prior_incomplete_evidence(journal, &plan.identity, &rerun, root, test_git())
            .is_err()
    );
    Ok(())
}

pub(super) fn verify_prior_run_cases(
    journal: &mut Registry,
    plan: &JournalReceipt,
    root: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut current = plan.clone();
    current.identity.run_id = 43;
    current.predecessor = Some(plan.identity.clone());
    current.identity.workflow_sha = "c".repeat(40);
    current.workflow_run_url = "https://github.com/example/demo-crate/actions/runs/43".into();
    let prior =
        inspect_prior_incomplete_evidence(journal, &plan.identity, &current, root, test_git())?;
    assert_eq!(&prior.receipt, plan);
    verify_chain_and_rerun(journal, plan, &current, root)?;
    let history = inspect_prior_history(journal, &plan.identity, &current, root, test_git())?;
    assert_eq!(history.mutations.len(), 0);
    journal.inject_after_list.set(true);
    assert!(inspect_prior_history(journal, &plan.identity, &current, root, test_git()).is_err());
    let inserted = format!(
        "refs/tags/velnor-receipt/{}/{}/operation-1",
        plan.identity.run_id, plan.identity.run_attempt
    );
    assert_eq!(journal.refs.borrow().len(), 2);
    assert_eq!(journal.refs.borrow().get(&inserted), Some(&"a".repeat(40)));
    journal.refs.borrow_mut().remove(&inserted);
    let gap = format!(
        "refs/tags/velnor-receipt/{}/{}/operation-2",
        plan.identity.run_id, plan.identity.run_attempt
    );
    journal
        .refs
        .borrow_mut()
        .insert(gap.clone(), "a".repeat(40));
    assert!(inspect_prior_history(journal, &plan.identity, &current, root, test_git()).is_err());
    journal.refs.borrow_mut().remove(&gap);
    let malformed = format!(
        "refs/tags/velnor-receipt/{}/{}/operation-01",
        plan.identity.run_id, plan.identity.run_attempt
    );
    journal
        .refs
        .borrow_mut()
        .insert(malformed.clone(), "a".repeat(40));
    assert!(inspect_prior_history(journal, &plan.identity, &current, root, test_git()).is_err());
    journal.refs.borrow_mut().remove(&malformed);
    for invalid in ["intent", "owner", "source", "repo", "run", "predecessor"] {
        let mut forged = current.clone();
        match invalid {
            "intent" => forged.base_intent_sha256 = "f".repeat(64),
            "owner" => {
                let crate::crate_release_journal::JournalOwnership::Existing { owners } =
                    &mut forged.packages[0].owners
                else {
                    panic!("expected owners")
                };
                owners[0].id += 1;
            }
            "source" => forged.source_files[0].sha256 = "f".repeat(64),
            "repo" => forged.identity.repository = "tailrocks/other".into(),
            "run" => forged.identity.run_id = plan.identity.run_id,
            "predecessor" => forged.predecessor = None,
            _ => return Err("bad fixture".into()),
        }
        assert!(
            inspect_prior_incomplete_evidence(journal, &plan.identity, &forged, root, test_git())
                .is_err()
        );
    }
    assert_eq!(journal.refs.borrow().len(), 1);
    journal.reject_access = Some(());
    assert!(
        inspect_prior_incomplete_evidence(journal, &plan.identity, &current, root, test_git())
            .is_err()
    );
    journal.reject_access = None;
    let mut missing = plan.identity.clone();
    missing.run_id = 44;
    assert!(
        inspect_prior_incomplete_evidence(journal, &missing, &current, root, test_git()).is_err()
    );
    finish_prior_run(journal, plan, &current, root)
}

pub(super) fn finish_prior_run(
    journal: &Registry,
    plan: &JournalReceipt,
    current: &JournalReceipt,
    root: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    verify_prior_operation_chain(journal, plan, current, root)?;
    let mut result = completion("0.1.1");
    result.packages[0]
        .tag_target
        .clone_from(&plan.identity.source_sha);
    record_journal(
        journal,
        &plan.identity,
        JournalStage::Completed,
        plan,
        Some(&result),
    )?;
    assert!(
        inspect_prior_incomplete_evidence(journal, &plan.identity, current, root, test_git())
            .is_err()
    );
    Ok(())
}

pub(super) fn verify_prior_operation_chain(
    journal: &Registry,
    plan: &JournalReceipt,
    current: &JournalReceipt,
    root: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    use velnor_actions_orchestrator::crate_release_journal::{
        JournalMutation, JournalMutationKind, JournalMutationPhase, JournalMutationResult,
        record_journal_mutation,
    };
    let mut mutation = JournalMutation {
        archive_sha256: None,
        semver_evidence: Some(
            crate::release::crate_release_semver::SemverEvidence::NoBaseline {
                checker_sha256: velnor_actions_mise::catalog::qualified_cargo_semver_checks(
                    "ubuntu-26.04",
                )
                .expect("qualified checker")
                .executable_sha256
                .into(),
            },
        ),
        sequence: 1,
        package: "demo-crate".into(),
        version: "0.1.1".into(),
        kind: JournalMutationKind::Upload,
        phase: JournalMutationPhase::Intended,
    };
    record_journal_mutation(journal, &plan.identity, plan, &mutation)?;
    let refs_before = journal.refs.borrow().len();
    let history = inspect_prior_history(journal, &plan.identity, current, root, test_git())?;
    assert_eq!(history.mutations, vec![mutation.clone()]);
    assert_eq!(journal.refs.borrow().len(), refs_before);
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
    record_journal_mutation(journal, &plan.identity, plan, &mutation)?;
    let history = inspect_prior_history(journal, &plan.identity, current, root, test_git())?;
    assert_eq!(history.mutations.len(), 2);
    assert_eq!(history.mutations[1], mutation);
    Ok(())
}

fn verify_long_retry_history(
    plan: &JournalReceipt,
    root: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    for count in [33_u64, 64, 96] {
        let journal = Registry::default();
        let mut cursor = plan.clone();
        cursor.predecessor = None;
        for offset in 0..count {
            cursor.identity.run_id = 100 + offset;
            cursor.workflow_run_url = format!(
                "https://github.com/{}/actions/runs/{}",
                cursor.identity.repository, cursor.identity.run_id
            );
            seed_started_record(&journal, &cursor.identity, &cursor)?;
            cursor.predecessor = Some(cursor.identity.clone());
        }
        cursor.identity.run_id += 1;
        cursor.workflow_run_url = format!(
            "https://github.com/{}/actions/runs/{}",
            cursor.identity.repository, cursor.identity.run_id
        );
        let chain = super::super::inspect_prior_chain(&journal, &cursor, root, test_git())?;
        assert_eq!(u64::try_from(chain.len())?, count);
        assert!(chain.iter().all(|prior| prior.mutations.is_empty()));
        assert_eq!(
            chain
                .first()
                .ok_or("missing first prior history")?
                .start
                .receipt
                .identity
                .run_id,
            99 + count
        );
        assert_eq!(
            chain
                .last()
                .ok_or("missing last prior history")?
                .start
                .receipt
                .identity
                .run_id,
            100
        );
    }
    Ok(())
}

fn verify_history_source_change(
    plan: &JournalReceipt,
    root: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let journal = Registry::default();
    seed_started_record(&journal, &plan.identity, plan)?;
    let mut current = plan.clone();
    current.identity.run_id += 1;
    current.predecessor = Some(plan.identity.clone());
    current.workflow_run_url = format!(
        "https://github.com/{}/actions/runs/{}",
        current.identity.repository, current.identity.run_id
    );
    let path = root.join("AGENTS.md");
    let original = std::fs::read(&path)?;
    *journal.change_source_after_list.borrow_mut() = Some(path.clone());
    let result = super::super::inspect_prior_chain(&journal, &current, root, test_git());
    std::fs::write(path, original)?;
    assert!(
        matches!(&result, Err(OrchestratorError::Internal { problem }) if problem == "crate_release_source_content_mismatch"),
        "source-change error: {:?}",
        result.as_ref().err()
    );
    assert_eq!(
        super::super::inspect_prior_chain(&journal, &current, root, test_git())?.len(),
        1
    );
    assert_eq!(journal.refs.borrow().len(), 1);
    Ok(())
}

fn verify_history_snapshot(
    plan: &JournalReceipt,
    root: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    use std::{
        rc::Rc,
        time::{Duration, Instant},
    };
    use velnor_actions_mise::CheckDeadline;
    let journal = Registry::default();
    seed_started_record(&journal, &plan.identity, plan)?;
    let mut current = plan.clone();
    current.identity.run_id += 1;
    current.predecessor = Some(plan.identity.clone());
    current.workflow_run_url = format!(
        "https://github.com/{}/actions/runs/{}",
        current.identity.repository, current.identity.run_id
    );
    let now = Rc::new(Cell::new(Instant::now()));
    let clock = now.clone();
    CheckDeadline::with_clock(
        move || clock.get(),
        || -> Result<(), Box<dyn std::error::Error>> {
            let deadline = CheckDeadline::after(Duration::from_secs(30))?;
            let snapshot = super::super::walk_prior_history_until(
                &journal,
                &current,
                root,
                test_git(),
                None,
                deadline,
                |_| Ok(()),
            )?;
            snapshot.revalidate(&journal, &current)?;
            let refs = journal.refs.borrow().clone();
            let reference = format!(
                "refs/tags/velnor-receipt/{}/{}/publication-started",
                plan.identity.run_id, plan.identity.run_attempt
            );
            journal.refs.borrow_mut().insert(reference, "a".repeat(40));
            assert!(snapshot.revalidate(&journal, &current).is_err());
            *journal.refs.borrow_mut() = refs;
            snapshot.revalidate(&journal, &current)?;
            now.set(now.get() + Duration::from_secs(31));
            assert!(snapshot.revalidate(&journal, &current).is_err());
            assert!(snapshot.matches(&snapshot).is_err());
            assert_eq!(journal.refs.borrow().len(), 1);
            Ok(())
        },
    )
}
