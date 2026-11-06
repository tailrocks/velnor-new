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
    let chained_journal = Registry::default();
    let mut older = plan.clone();
    older.identity.run_id -= 1;
    older.workflow_run_url = format!(
        "https://github.com/{}/actions/runs/{}",
        older.identity.repository, older.identity.run_id
    );
    record_source_bound_start(&chained_journal, &older.identity, &older, root, test_git())?;
    let mut chained = plan.clone();
    chained.predecessor = Some(older.identity.clone());
    record_source_bound_start(
        &chained_journal,
        &chained.identity,
        &chained,
        root,
        test_git(),
    )?;
    assert_eq!(
        super::super::inspect_prior_chain(&chained_journal, current, root, test_git())?.len(),
        2
    );
    let cyclic_journal = Registry::default();
    older.predecessor = Some(chained.identity.clone());
    record_source_bound_start(&cyclic_journal, &older.identity, &older, root, test_git())?;
    record_source_bound_start(
        &cyclic_journal,
        &chained.identity,
        &chained,
        root,
        test_git(),
    )?;
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
