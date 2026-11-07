// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{Github, JournalRegistryFake, Registry, fixture_journal_plan, test_git};
use crate::crate_release_controller::{CrateReleaseObservation, FrozenCompletion};
use crate::crate_release_journal::{JournalStage, inspect_journal};

#[test]
fn predecessor_history_requires_qualified_source_context() -> Result<(), Box<dyn std::error::Error>>
{
    let temp = tempfile::tempdir()?;
    let source = temp.path();
    let (intent, request, github, registry) = super::make_fixture(source)?;
    github.approved.set(true);
    github.tag_exists.set(true);
    github.release_variant.set(0);
    registry.absent.set(false);
    *registry.archive.borrow_mut() = super::fixture_archive(source)?;
    let admission = super::prepare_crate_release(&intent, &request, &github, &registry, source)?;
    let (_, mut receipt) = fixture_journal_plan(source, &intent, &request, &admission)?;
    let mut prior = receipt.identity.clone();
    prior.run_id -= 1;
    receipt.predecessor = Some(prior);
    let deadline = velnor_actions_mise::CheckDeadline::after(std::time::Duration::from_secs(30))?;
    let result = crate::release::crate_release_direct_history::HistoryContext::admitted(
        source,
        &intent,
        &request,
        test_git(),
        &receipt,
        deadline,
    );
    assert!(matches!(
        result,
        Err(crate::OrchestratorError::Internal { problem })
            if problem == "crate_release_history_custody_required"
    ));
    assert_eq!(github.writes.get(), 0);
    Ok(())
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
            crate::release::package_release_signer::fixture_measured_tools(
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
            crate::release::package_release_signer::fixture_measured_tools(
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
