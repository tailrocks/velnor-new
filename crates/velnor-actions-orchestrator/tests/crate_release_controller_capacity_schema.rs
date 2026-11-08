// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn near_full_recoverable_history_stops_before_next_package()
-> Result<(), Box<dyn std::error::Error>> {
    let (temp, intent, request, github, first_registry) = make_workspace_order_fixture()?;
    let root = temp.path();
    let initial = prepare_crate_release(&intent, &request, &github, &first_registry, root)?;
    let archive_dir = tempfile::tempdir()?;
    let archive = crate::crate_release_controller::package_unprivileged(
        root,
        &intent,
        &initial.packages[0].source,
        archive_dir.path(),
        &request.source_sha,
        false,
        test_git(),
    )?;
    *first_registry.archive.borrow_mut() = archive;
    first_registry.visible.set(true);
    first_registry.index_visible.set(true);
    let registry = super::MixedRegistry {
        first: first_registry,
        second: RefCell::new(None),
    };
    let github = workspace_github(github);
    github.tags[0].set(true);
    github.releases[0].set(true);
    let (identity, mut receipt) = fixture_journal_plan(root, &intent, &request, &initial)?;
    set_workspace_journal_packages(&mut receipt, &initial);
    let journal = JournalRegistryFake::default();
    seed_started_record(&journal, &identity, &receipt)?;
    let package = &receipt.packages[0];
    for sequence in 1..=998 {
        let phase = if sequence % 2 == 1 {
            JournalMutationPhase::Intended
        } else {
            JournalMutationPhase::Observed {
                result: JournalMutationResult::Tag {
                    name: package.tag.clone(),
                    target_sha: identity.source_sha.clone(),
                },
            }
        };
        let mutation = JournalMutation {
            archive_sha256: None,
            sequence,
            package: package.name.clone(),
            version: package.version.clone(),
            kind: JournalMutationKind::CreateTag,
            semver_evidence: None,
            phase,
        };
        crate::crate_release_journal::fixture_storage::seed_mutation_record(
            &journal, &identity, &receipt, &mutation,
        )?;
    }
    let (history, completed) =
        crate::crate_release_journal::inspect_recoverable_history(&journal, &identity, &receipt)?;
    assert_eq!(history.len(), 998);
    assert!(completed.is_none());
    journal.write_calls.set(0);
    let input = CrateReleaseObservation {
        source_execution: &crate::UnprivilegedSourceExecution,
        git: test_git(),
        journal: &journal,
        identity: &identity,
        journal_receipt: &receipt,
        admission: &initial,
        intent: &intent,
        request: &request,
        github: &github,
        registry: &registry,
        source: root,
        sequence: 1,
    };
    assert_capacity_stops_before_callbacks(&input, &journal);
    assert!(github.writes.borrow().is_empty());
    Ok(())
}

#[test]
fn too_many_packaged_features_stop_before_registry_mutation()
-> Result<(), Box<dyn std::error::Error>> {
    use std::fmt::Write as _;
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    let (mut intent, mut request, mut github, registry) = make_fixture(root)?;
    let manifest = root.join("Cargo.toml");
    let mut text = fs::read_to_string(&manifest)?;
    for index in 0..10_000 {
        writeln!(&mut text, "f{index:05}=[]")?;
    }
    fs::write(manifest, text)?;
    let sha = commit_paths(root)?;
    intent.recovery.as_mut().ok_or("recovery")?.source_sha = sha.clone();
    request.source_sha = sha.clone();
    request.recovery_sha = Some(sha.clone());
    github.sha = sha;
    github.approved.set(true);
    let admission = prepare_crate_release(&intent, &request, &github, &registry, root)?;
    let (identity, receipt) = fixture_journal_plan(root, &intent, &request, &admission)?;
    let journal = JournalRegistryFake::default();
    seed_started_record(&journal, &identity, &receipt)?;
    journal.write_calls.set(0);
    let input = CrateReleaseObservation {
        source_execution: &crate::UnprivilegedSourceExecution,
        git: test_git(),
        journal: &journal,
        identity: &identity,
        journal_receipt: &receipt,
        admission: &admission,
        intent: &intent,
        request: &request,
        github: &github,
        registry: &registry,
        source: root,
        sequence: 1,
    };
    assert_capacity_stops_before_callbacks(&input, &journal);
    assert_eq!(github.writes.get(), 0);
    Ok(())
}

#[test]
fn too_many_packaged_files_stop_before_registry_mutation() -> Result<(), Box<dyn std::error::Error>>
{
    use std::os::unix::fs::symlink;
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    let (mut intent, mut request, mut github, registry) = make_fixture(root)?;
    let assets = root.join("assets");
    fs::create_dir(&assets)?;
    fs::write(assets.join("target"), b"x")?;
    for index in 0..99_996 {
        symlink("target", assets.join(format!("f{index:05}")))?;
    }
    let sha = commit_paths(root)?;
    intent.recovery.as_mut().ok_or("recovery")?.source_sha = sha.clone();
    request.source_sha = sha.clone();
    request.recovery_sha = Some(sha.clone());
    github.sha = sha;
    github.approved.set(true);
    let admission = prepare_crate_release(&intent, &request, &github, &registry, root)?;
    let (identity, receipt) = fixture_journal_plan(root, &intent, &request, &admission)?;
    assert_eq!(receipt.source_files.len(), 100_000);
    assert!(package_files(root, "demo")?.len() > 100_000);
    let journal = JournalRegistryFake::default();
    seed_started_record(&journal, &identity, &receipt)?;
    journal.write_calls.set(0);
    let input = CrateReleaseObservation {
        source_execution: &crate::UnprivilegedSourceExecution,
        git: test_git(),
        journal: &journal,
        identity: &identity,
        journal_receipt: &receipt,
        admission: &admission,
        intent: &intent,
        request: &request,
        github: &github,
        registry: &registry,
        source: root,
        sequence: 1,
    };
    assert_capacity_stops_before_callbacks(&input, &journal);
    assert_eq!(github.writes.get(), 0);
    Ok(())
}
