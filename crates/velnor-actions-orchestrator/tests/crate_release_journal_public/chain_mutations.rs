// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;
use velnor_actions_orchestrator::crate_release_journal::{
    JournalMutation, JournalMutationKind, JournalMutationPhase, JournalMutationResult,
    record_journal_mutation,
};

#[test]
fn published_retry_history_preserves_all_uploads() -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    let git = |args: &[&str]| -> Result<String, Box<dyn std::error::Error>> {
        let output = std::process::Command::new("/usr/bin/git")
            .current_dir(root)
            .args(args)
            .output()?;
        if !output.status.success() {
            return Err("git fixture failed".into());
        }
        Ok(String::from_utf8(output.stdout)?.trim().to_owned())
    };
    git(&["init", "-q"])?;
    git(&["config", "user.name", "Fixture"])?;
    git(&["config", "user.email", "fixture@example.invalid"])?;
    std::fs::write(root.join("AGENTS.md"), "approved")?;
    git(&["add", "."])?;
    git(&["-c", "commit.gpgsign=false", "commit", "-qm", "fixture"])?;
    let sha = git(&["rev-parse", "HEAD"])?;
    let mut plan = receipt("0.1.1");
    plan.identity.source_sha.clone_from(&sha);
    plan.source_files = crate::journal_source_inventory(root, &sha, test_git())?;
    verify_published_retry_history(&plan, root)
}

fn verify_published_retry_history(
    base: &JournalReceipt,
    root: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let journal = Registry::default();
    let plan = seed_history(base, &journal)?;
    let refs_before = journal.refs.borrow().clone();
    let blobs_before = journal.blobs.borrow().clone();
    let tags_before = journal.tags.borrow().clone();
    let writes_before = journal.write_calls.get();
    let start = std::time::Instant::now();
    let deadline = velnor_actions_mise::CheckDeadline::after(std::time::Duration::from_secs(600))?;
    let mut chain = Vec::new();
    let snapshot = super::super::walk_prior_history_until(
        &journal,
        &plan,
        root,
        test_git(),
        None,
        deadline,
        |history| {
            chain.push(history);
            Ok(())
        },
    )?;
    let elapsed = start.elapsed();
    assert_eq!(chain.len(), 33);
    for (position, prior) in chain.iter().enumerate() {
        let index = 32 - position;
        assert_eq!(prior.start.receipt.packages, plan.packages);
        assert_eq!(
            prior.start.receipt.identity.run_id,
            100 + u64::try_from(index)?
        );
        assert_eq!(prior.mutations.len(), 2);
        assert_eq!(prior.mutations[0].package, format!("demo-{index:02}"));
        assert_eq!(prior.mutations[1].package, format!("demo-{index:02}"));
        assert!(matches!(
            prior.mutations[0].phase,
            JournalMutationPhase::Intended
        ));
        assert!(matches!(
            prior.mutations[1].phase,
            JournalMutationPhase::Observed { .. }
        ));
    }
    assert_eq!(*journal.refs.borrow(), refs_before);
    assert!(elapsed < std::time::Duration::from_secs(600));
    snapshot.revalidate(&journal, &plan)?;
    let middle = "refs/tags/velnor-receipt/116/1/operation-2";
    journal
        .refs
        .borrow_mut()
        .insert(middle.into(), "a".repeat(40));
    let refs_after_drift = journal.refs.borrow().clone();
    assert!(snapshot.revalidate(&journal, &plan).is_err());
    assert_eq!(*journal.refs.borrow(), refs_after_drift);
    *journal.refs.borrow_mut() = refs_before.clone();
    snapshot.revalidate(&journal, &plan)?;
    let file = root.join("AGENTS.md");
    let original = std::fs::read(&file)?;
    *journal.change_source_after_list.borrow_mut() = Some(file.clone());
    let result = super::super::inspect_prior_chain(&journal, &plan, root, test_git());
    std::fs::write(file, original)?;
    assert!(result.is_err());
    assert_eq!(*journal.refs.borrow(), refs_before);
    assert_eq!(*journal.blobs.borrow(), blobs_before);
    assert_eq!(*journal.tags.borrow(), tags_before);
    assert_eq!(journal.write_calls.get(), writes_before);
    Ok(())
}

fn seed_history(
    base: &JournalReceipt,
    journal: &Registry,
) -> Result<JournalReceipt, Box<dyn std::error::Error>> {
    let mut plan = base.clone();
    let template = plan.packages[0].clone();
    let semver = plan.semver_evidence["demo-crate"].clone();
    plan.packages.clear();
    plan.semver_evidence.clear();
    for index in 0..33_u64 {
        let name = format!("demo-{index:02}");
        let mut package = template.clone();
        package.name.clone_from(&name);
        package.manifest = format!("crates/{name}/Cargo.toml");
        package.tag = format!("{name}-v{}", package.version);
        plan.semver_evidence.insert(name, semver.clone());
        plan.packages.push(package);
    }
    plan.predecessor = None;
    for index in 0..33_u64 {
        plan.identity.run_id = 100 + index;
        plan.workflow_run_url = format!(
            "https://github.com/{}/actions/runs/{}",
            plan.identity.repository, plan.identity.run_id
        );
        seed_started_record(journal, &plan.identity, &plan)?;
        let package = &plan.packages[usize::try_from(index)?];
        let mut mutation = JournalMutation {
            archive_sha256: None,
            semver_evidence: Some(semver.clone()),
            sequence: 1,
            package: package.name.clone(),
            version: package.version.clone(),
            kind: JournalMutationKind::Upload,
            phase: JournalMutationPhase::Intended,
        };
        record_journal_mutation(journal, &plan.identity, &plan, &mutation)?;
        mutation.sequence = 2;
        mutation.semver_evidence = None;
        mutation.phase = JournalMutationPhase::Observed {
            result: JournalMutationResult::Upload {
                registry_checksum: "d".repeat(64),
                downloaded_checksum: "d".repeat(64),
                owners: journal_existing_owners(&package.owners),
                features: BTreeMap::new(),
                files: completion("0.1.1").packages[0].files.clone(),
            },
        };
        record_journal_mutation(journal, &plan.identity, &plan, &mutation)?;
        plan.predecessor = Some(plan.identity.clone());
    }
    plan.identity.run_id = 133;
    plan.workflow_run_url = format!(
        "https://github.com/{}/actions/runs/{}",
        plan.identity.repository, plan.identity.run_id
    );
    Ok(plan)
}
