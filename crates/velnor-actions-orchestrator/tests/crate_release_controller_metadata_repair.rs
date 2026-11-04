// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[test]
fn metadata_repair_journals_each_create_before_and_after_write()
-> Result<(), Box<dyn std::error::Error>> {
    use crate::crate_release_controller::{CrateReleaseObservation, repair_verified_metadata};
    use crate::crate_release_journal::{
        JournalMutationKind, JournalMutationPhase, inspect_journal_mutation,
        record_source_bound_start,
    };
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    let (intent, request, github, registry) = make_fixture(root)?;
    github.approved.set(true);
    let admission = prepare_crate_release(&intent, &request, &github, &registry, root)?;
    let (identity, plan) = fixture_journal_plan(root, &intent, &request, &admission)?;
    let journal = std::rc::Rc::new(JournalRegistryFake::default());
    record_source_bound_start(journal.as_ref(), &identity, &plan, root, test_git())?;
    *registry.archive.borrow_mut() = fixture_archive(root)?;
    registry.absent.set(false);
    github.release_variant.set(4);
    let observed_journal = journal.clone();
    let observed_identity = identity.clone();
    *github.before_metadata_write.borrow_mut() = Some(Box::new(move |sequence| {
        let intended =
            inspect_journal_mutation(observed_journal.as_ref(), &observed_identity, sequence)
                .expect("read intention")
                .expect("intention exists");
        assert!(matches!(intended.phase, JournalMutationPhase::Intended));
        assert!(
            inspect_journal_mutation(observed_journal.as_ref(), &observed_identity, sequence + 1)
                .expect("read observation")
                .is_none()
        );
    }));
    let input = CrateReleaseObservation {
        git: test_git(),
        journal: journal.as_ref(),
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
    reject_invalid_metadata_prerequisites(&input);
    assert_eq!(github.writes.get(), 0);
    assert!(inspect_journal_mutation(journal.as_ref(), &identity, 1)?.is_none());
    repair_verified_metadata(&input, "demo", JournalMutationKind::CreateTag)?;
    assert_eq!(github.writes.get(), 1);
    assert!(matches!(
        inspect_journal_mutation(journal.as_ref(), &identity, 1)?
            .ok_or("intent")?
            .phase,
        JournalMutationPhase::Intended
    ));
    assert!(matches!(
        inspect_journal_mutation(journal.as_ref(), &identity, 2)?
            .ok_or("observed")?
            .phase,
        JournalMutationPhase::Observed { .. }
    ));
    let release = CrateReleaseObservation {
        sequence: 3,
        ..input
    };
    repair_verified_metadata(&release, "demo", JournalMutationKind::CreateRelease)?;
    assert_eq!(github.writes.get(), 2);
    assert!(matches!(
        inspect_journal_mutation(journal.as_ref(), &identity, 3)?
            .ok_or("intent")?
            .phase,
        JournalMutationPhase::Intended
    ));
    assert!(matches!(
        inspect_journal_mutation(journal.as_ref(), &identity, 4)?
            .ok_or("observed")?
            .phase,
        JournalMutationPhase::Observed { .. }
    ));
    Ok(())
}

#[test]
fn ambiguous_metadata_write_stops_with_durable_unresolved_intent()
-> Result<(), Box<dyn std::error::Error>> {
    use crate::crate_release_controller::{CrateReleaseObservation, repair_verified_metadata};
    use crate::crate_release_journal::{
        JournalMutationKind, JournalMutationPhase, inspect_journal_mutation,
        record_source_bound_start,
    };
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    let (intent, request, github, registry) = make_fixture(root)?;
    github.approved.set(true);
    let admission = prepare_crate_release(&intent, &request, &github, &registry, root)?;
    let (identity, plan) = fixture_journal_plan(root, &intent, &request, &admission)?;
    let journal = JournalRegistryFake::default();
    record_source_bound_start(&journal, &identity, &plan, root, test_git())?;
    *registry.archive.borrow_mut() = fixture_archive(root)?;
    registry.absent.set(false);
    github.release_variant.set(4);
    github.write_error.set(3);
    let input = CrateReleaseObservation {
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
    assert!(repair_verified_metadata(&input, "demo", JournalMutationKind::CreateTag).is_err());
    assert_eq!(github.writes.get(), 1);
    assert!(matches!(
        inspect_journal_mutation(&journal, &identity, 1)?
            .ok_or("intent")?
            .phase,
        JournalMutationPhase::Intended
    ));
    assert!(inspect_journal_mutation(&journal, &identity, 2)?.is_none());
    assert!(repair_verified_metadata(&input, "demo", JournalMutationKind::CreateTag).is_err());
    assert_eq!(github.writes.get(), 1);
    Ok(())
}

#[test]
fn metadata_intent_requires_fresh_approval_after_state_observation()
-> Result<(), Box<dyn std::error::Error>> {
    use crate::crate_release_controller::{CrateReleaseObservation, repair_verified_metadata};
    use crate::crate_release_journal::{
        JournalMutationKind, inspect_journal_mutation, record_source_bound_start,
    };
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    let (intent, request, github, registry) = make_fixture(root)?;
    github.approved.set(true);
    let admission = prepare_crate_release(&intent, &request, &github, &registry, root)?;
    let (identity, plan) = fixture_journal_plan(root, &intent, &request, &admission)?;
    let journal = JournalRegistryFake::default();
    record_source_bound_start(&journal, &identity, &plan, root, test_git())?;
    *registry.archive.borrow_mut() = fixture_archive(root)?;
    registry.absent.set(false);
    github.revoke_on_tag_read.set(true);
    let input = CrateReleaseObservation {
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
    assert!(repair_verified_metadata(&input, "demo", JournalMutationKind::CreateTag).is_err());
    assert_eq!(github.writes.get(), 0);
    assert!(
        inspect_journal_mutation(&journal, &identity, 1)?.is_none(),
        "revoked approval must prevent recording a new intent"
    );
    Ok(())
}

#[test]
fn metadata_repair_stops_if_tag_or_registry_changes_after_intent()
-> Result<(), Box<dyn std::error::Error>> {
    use crate::crate_release_controller::{CrateReleaseObservation, repair_verified_metadata};
    use crate::crate_release_journal::{
        JournalMutationKind, inspect_journal_mutation, record_source_bound_start,
    };
    for (kind, change_tag) in [
        (JournalMutationKind::CreateTag, true),
        (JournalMutationKind::CreateTag, false),
        (JournalMutationKind::CreateRelease, false),
    ] {
        let temp = tempfile::tempdir()?;
        let root = temp.path();
        let (intent, request, github, registry) = make_fixture(root)?;
        let github = std::rc::Rc::new(github);
        let registry = std::rc::Rc::new(registry);
        github.approved.set(true);
        let admission =
            prepare_crate_release(&intent, &request, github.as_ref(), registry.as_ref(), root)?;
        let (identity, plan) = fixture_journal_plan(root, &intent, &request, &admission)?;
        let journal = JournalRegistryFake::default();
        record_source_bound_start(&journal, &identity, &plan, root, test_git())?;
        *registry.archive.borrow_mut() = fixture_archive(root)?;
        registry.absent.set(false);
        if kind == JournalMutationKind::CreateRelease {
            github.tag_exists.set(true);
            github.release_variant.set(4);
        }
        let observed_github = github.clone();
        let observed_registry = registry.clone();
        *journal.after_ref_created.borrow_mut() = Some(Box::new(move |reference| {
            if reference.ends_with("operation-1") {
                if change_tag {
                    observed_github.tag_exists.set(true);
                } else {
                    observed_registry.absent.set(true);
                }
            }
        }));
        let input = CrateReleaseObservation {
            git: test_git(),
            journal: &journal,
            identity: &identity,
            journal_receipt: &plan,
            admission: &admission,
            intent: &intent,
            request: &request,
            github: github.as_ref(),
            registry: registry.as_ref(),
            source: root,
            sequence: 1,
        };
        assert!(repair_verified_metadata(&input, "demo", kind).is_err());
        assert_eq!(github.writes.get(), 0);
        assert!(inspect_journal_mutation(&journal, &identity, 1)?.is_some());
        assert!(inspect_journal_mutation(&journal, &identity, 2)?.is_none());
    }
    Ok(())
}

#[test]
fn metadata_repair_stops_if_approval_changes_after_intent() -> Result<(), Box<dyn std::error::Error>>
{
    use crate::crate_release_controller::{CrateReleaseObservation, repair_verified_metadata};
    use crate::crate_release_journal::{
        JournalMutationKind, JournalMutationPhase, inspect_journal_mutation,
        record_source_bound_start,
    };
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    let (intent, request, github, registry) = make_fixture(root)?;
    let github = std::rc::Rc::new(github);
    github.approved.set(true);
    let admission = prepare_crate_release(&intent, &request, github.as_ref(), &registry, root)?;
    let (identity, plan) = fixture_journal_plan(root, &intent, &request, &admission)?;
    let journal = JournalRegistryFake::default();
    record_source_bound_start(&journal, &identity, &plan, root, test_git())?;
    *registry.archive.borrow_mut() = fixture_archive(root)?;
    registry.absent.set(false);
    let observed_github = github.clone();
    *journal.after_ref_created.borrow_mut() = Some(Box::new(move |reference| {
        if reference.ends_with("operation-1") {
            observed_github.approved.set(false);
        }
    }));
    let input = CrateReleaseObservation {
        git: test_git(),
        journal: &journal,
        identity: &identity,
        journal_receipt: &plan,
        admission: &admission,
        intent: &intent,
        request: &request,
        github: github.as_ref(),
        registry: &registry,
        source: root,
        sequence: 1,
    };
    assert!(repair_verified_metadata(&input, "demo", JournalMutationKind::CreateTag).is_err());
    assert_eq!(github.writes.get(), 0);
    assert!(matches!(
        inspect_journal_mutation(&journal, &identity, 1)?
            .ok_or("intent")?
            .phase,
        JournalMutationPhase::Intended
    ));
    assert!(inspect_journal_mutation(&journal, &identity, 2)?.is_none());
    Ok(())
}

