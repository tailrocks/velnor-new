// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{Github, JournalRegistryFake, Registry, fixture_journal_plan, test_git};
use crate::crate_release_controller::{CrateReleaseObservation, FrozenCompletion};
use crate::crate_release_journal::{JournalStage, inspect_journal, record_source_bound_start};

#[test]
fn completion_rejects_predecessor_drift_after_source_freeze()
-> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let source = temp.path();
    let (intent, request, github, registry) = super::make_fixture(source)?;
    github.approved.set(true);
    github.tag_exists.set(true);
    github.release_variant.set(0);
    registry.absent.set(false);
    *registry.archive.borrow_mut() = super::fixture_archive(source)?;
    let admission = super::prepare_crate_release(&intent, &request, &github, &registry, source)?;
    let (identity, receipt) = fixture_journal_plan(source, &intent, &request, &admission)?;
    let journal = JournalRegistryFake::default();
    record_source_bound_start(&journal, &identity, &receipt, source, test_git())?;
    let input = CrateReleaseObservation {
        source_execution: &crate::UnprivilegedSourceExecution,
        journal: &journal,
        identity: &identity,
        journal_receipt: &receipt,
        admission: &admission,
        intent: &intent,
        request: &request,
        github: &github,
        registry: &registry,
        source,
        git: test_git(),
        sequence: 1,
    };
    reject_prior_history_drift(&input)
}

pub(super) fn reject_prior_history_drift(
    input: &CrateReleaseObservation<'_, JournalRegistryFake, Github, Registry>,
) -> Result<(), Box<dyn std::error::Error>> {
    let journal = JournalRegistryFake::default();
    let mut prior = input.journal_receipt.clone();
    prior.identity.run_id -= 1;
    prior.workflow_run_url = format!(
        "https://github.com/{}/actions/runs/{}",
        prior.identity.repository, prior.identity.run_id
    );
    record_source_bound_start(&journal, &prior.identity, &prior, input.source, test_git())?;
    let mut intent = input.intent.clone();
    intent.journal_predecessor = Some(velnor_actions_contract::config::CrateReleasePredecessor {
        run_id: prior.identity.run_id,
        run_attempt: prior.identity.run_attempt,
        workflow_sha: prior.identity.workflow_sha.clone(),
    });
    let mut admission = input.admission.clone();
    admission.intent = intent.clone();
    let (identity, mut receipt) =
        fixture_journal_plan(input.source, &intent, input.request, &admission)?;
    receipt.predecessor = Some(prior.identity.clone());
    record_source_bound_start(&journal, &identity, &receipt, input.source, test_git())?;
    let observation = CrateReleaseObservation {
        journal: &journal,
        identity: &identity,
        journal_receipt: &receipt,
        admission: &admission,
        intent: &intent,
        ..*input
    };
    let original_refs = journal.refs.borrow().clone();
    FrozenCompletion::prepare_unprivileged(
        &observation,
        crate::package_release_signer::fixture_measured_tools(receipt.expected_tools.clone()),
        &crate::UnprivilegedSourceExecution,
    )?
    .record_unprivileged(
        &crate::UnprivilegedSourceExecution,
        &journal,
        input.github,
        input.registry,
    )?;
    assert!(inspect_journal(&journal, &identity, JournalStage::Completed)?.is_some());
    *journal.refs.borrow_mut() = original_refs;
    for scenario in 0..3 {
        let frozen = FrozenCompletion::prepare_unprivileged(
            &observation,
            crate::package_release_signer::fixture_measured_tools(receipt.expected_tools.clone()),
            &crate::UnprivilegedSourceExecution,
        )?;
        let refs = journal.refs.borrow().clone();
        let before = journal.write_calls.get();
        let prefix = format!(
            "refs/tags/velnor-receipt/{}/{}",
            prior.identity.run_id, prior.identity.run_attempt
        );
        alter_prior_history(&journal, &prefix, scenario);
        let result = frozen.record_unprivileged(
            &crate::UnprivilegedSourceExecution,
            &journal,
            input.github,
            input.registry,
        );
        *journal.refs.borrow_mut() = refs;
        assert!(result.is_err(), "post-freeze predecessor drift {scenario}");
        assert_eq!(journal.write_calls.get(), before);
        assert!(inspect_journal(&journal, &identity, JournalStage::Completed)?.is_none());
    }
    Ok(())
}

fn alter_prior_history(journal: &JournalRegistryFake, prefix: &str, scenario: u8) {
    match scenario {
        0 => {
            journal
                .refs
                .borrow_mut()
                .remove(&format!("{prefix}/publication-started"));
        }
        1 => {
            journal
                .refs
                .borrow_mut()
                .insert(format!("{prefix}/operation-1"), "a".repeat(40));
        }
        _ => {
            journal
                .refs
                .borrow_mut()
                .insert(format!("{prefix}/reconciled"), "a".repeat(40));
        }
    }
}

pub(super) fn reject_post_freeze_drift(
    input: &crate::crate_release_controller::CrateReleaseObservation<
        '_,
        JournalRegistryFake,
        Github,
        Registry,
    >,
) -> Result<(), Box<dyn std::error::Error>> {
    for scenario in 0..8 {
        let frozen = crate::crate_release_controller::FrozenCompletion::prepare_unprivileged(
            input,
            crate::package_release_signer::fixture_measured_tools(
                input.journal_receipt.expected_tools.clone(),
            ),
            &crate::UnprivilegedSourceExecution,
        )?;
        let refs = input.journal.refs.borrow().clone();
        let archive = input.registry.archive.borrow().clone();
        let writes = input.journal.write_calls.get();
        match scenario {
            0 => input.github.approved.set(false),
            1 => input.github.tag_wrong.set(true),
            2 => input.github.release_variant.set(1),
            3 => input.registry.archive.borrow_mut().push(0),
            4 => input.registry.inconsistent.set(true),
            5 => {
                input
                    .journal
                    .refs
                    .borrow_mut()
                    .retain(|key, _| !key.ends_with("operation-6"));
            }
            6 => input.github.owner_match.set(false),
            _ => input
                .github
                .revoke_at_ruleset
                .set(input.github.ruleset_reads.get() + 1),
        }
        let result = frozen.record_unprivileged(
            &crate::UnprivilegedSourceExecution,
            input.journal,
            input.github,
            input.registry,
        );
        input.github.approved.set(true);
        input.github.tag_wrong.set(false);
        input.github.release_variant.set(0);
        input.github.owner_match.set(true);
        input.github.revoke_at_ruleset.set(0);
        input.registry.inconsistent.set(false);
        *input.registry.archive.borrow_mut() = archive;
        *input.journal.refs.borrow_mut() = refs;
        assert!(result.is_err(), "post-freeze drift scenario {scenario}");
        assert_eq!(
            input.journal.write_calls.get(),
            writes,
            "post-freeze drift must write no completion"
        );
    }
    Ok(())
}

pub(super) fn completion_recovers_lost_write_response(
    input: &CrateReleaseObservation<'_, JournalRegistryFake, Github, Registry>,
) -> Result<(), Box<dyn std::error::Error>> {
    let journal = JournalRegistryFake {
        refs: std::cell::RefCell::new(input.journal.refs.borrow().clone()),
        blobs: std::cell::RefCell::new(input.journal.blobs.borrow().clone()),
        tags: std::cell::RefCell::new(input.journal.tags.borrow().clone()),
        fail_ref_after_write: true,
        ..JournalRegistryFake::default()
    };
    let observation = CrateReleaseObservation {
        journal: &journal,
        ..*input
    };
    let freeze = || {
        FrozenCompletion::prepare_unprivileged(
            &observation,
            crate::package_release_signer::fixture_measured_tools(
                input.journal_receipt.expected_tools.clone(),
            ),
            &crate::UnprivilegedSourceExecution,
        )
    };
    let result = freeze()?.record_unprivileged(
        &crate::UnprivilegedSourceExecution,
        &journal,
        input.github,
        input.registry,
    );
    assert!(
        matches!(result, Err(crate::OrchestratorError::Internal { problem }) if problem == "crate_release_journal_ref_write_ambiguous")
    );
    let completed = inspect_journal(&journal, input.identity, JournalStage::Completed)?
        .ok_or("accepted completion not visible")?;
    let writes = journal.write_calls.get();
    let github_writes = input.github.writes.get();
    freeze()?.record_unprivileged(
        &crate::UnprivilegedSourceExecution,
        &journal,
        input.github,
        input.registry,
    )?;
    assert_eq!(journal.write_calls.get(), writes);
    assert_eq!(input.github.writes.get(), github_writes);
    assert_eq!(
        inspect_journal(&journal, input.identity, JournalStage::Completed)?.as_ref(),
        Some(&completed)
    );
    Ok(())
}
