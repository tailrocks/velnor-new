// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn journal_versions_match_crates_io_length_limit() {
    let version = format!("1.2.3-{}", "a".repeat(144));
    let remote = Registry::default();
    assert!(
        record_journal(
            &remote,
            &identity(),
            JournalStage::Started,
            &receipt(&version),
            None
        )
        .is_ok()
    );
    let too_long = format!("{version}a");
    assert!(
        record_journal(
            &Registry::default(),
            &identity(),
            JournalStage::Started,
            &receipt(&too_long),
            None
        )
        .is_err()
    );
}

#[test]
fn receipt_requires_cargo_version_and_exact_intent_tag() {
    let remote = Registry::default();
    let mut malformed = receipt("bogus");
    malformed.packages[0].tag = "demo-crate-vbogus".to_owned();
    assert!(
        record_journal(
            &remote,
            &identity(),
            JournalStage::Started,
            &malformed,
            None
        )
        .is_err()
    );
    let mut wrong_tag = receipt("0.1.1");
    wrong_tag.packages[0].tag = "other-v0.1.1".to_owned();
    assert!(
        record_journal(
            &remote,
            &identity(),
            JournalStage::Started,
            &wrong_tag,
            None
        )
        .is_err()
    );
}

#[test]
fn journal_absence_requires_authenticated_repository_access() {
    let remote = Registry::default();
    assert!(
        inspect_journal(&remote, &identity(), JournalStage::Started)
            .expect("observe state")
            .is_none()
    );
    assert!(remote.access_checks.get() >= 2);
    let denied = Registry {
        reject_access: Some(()),
        ..Registry::default()
    };
    assert!(inspect_journal(&denied, &identity(), JournalStage::Started).is_err());
}

#[test]
fn source_symlink_evidence_must_resolve_to_a_tracked_file() -> Result<(), Box<dyn std::error::Error>>
{
    let mut plan = receipt("0.1.1");
    plan.source_files.push(JournalFile {
        path: "AGENTS.md".to_owned(),
        sha256: "d".repeat(64),
        kind: JournalFileKind::Regular { executable: false },
    });
    plan.source_files.push(JournalFile {
        path: "CLAUDE.md".to_owned(),
        sha256: "a54ff182c7e8acf56acfd6e4b9c3ff41e2c41a31c9b211b2deb9df75d9a478f9".to_owned(),
        kind: JournalFileKind::Symlink {
            target: "AGENTS.md".to_owned(),
        },
    });
    record_journal(
        &Registry::default(),
        &identity(),
        JournalStage::Started,
        &plan,
        None,
    )?;
    plan.source_files.pop();
    plan.source_files.push(JournalFile {
        path: "CLAUDE.md".to_owned(),
        sha256: "a54ff182c7e8acf56acfd6e4b9c3ff41e2c41a31c9b211b2deb9df75d9a478f9".to_owned(),
        kind: JournalFileKind::Symlink {
            target: "missing".to_owned(),
        },
    });
    assert!(
        record_journal(
            &Registry::default(),
            &identity(),
            JournalStage::Started,
            &plan,
            None
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn source_symlink_rejects_escape_cycles_and_forged_hashes() -> Result<(), Box<dyn std::error::Error>>
{
    fn link(path: &str, target: &str) -> JournalFile {
        use sha2::{Digest as _, Sha256};
        use std::fmt::Write as _;
        let hash = Sha256::digest(target.as_bytes())
            .iter()
            .fold(String::new(), |mut out, byte| {
                write!(&mut out, "{byte:02x}").expect("writing to a string cannot fail");
                out
            });
        JournalFile {
            path: path.to_owned(),
            sha256: hash,
            kind: JournalFileKind::Symlink {
                target: target.to_owned(),
            },
        }
    }
    let mut plan = receipt("0.1.1");
    plan.source_files.push(link("docs/link", "../src/lib.rs"));
    record_journal(
        &Registry::default(),
        &identity(),
        JournalStage::Started,
        &plan,
        None,
    )?;
    for target in ["../../escape", "/etc/passwd", "untracked", "link"] {
        *plan.source_files.last_mut().ok_or("missing link")? = link("docs/link", target);
        assert!(
            record_journal(
                &Registry::default(),
                &identity(),
                JournalStage::Started,
                &plan,
                None
            )
            .is_err(),
            "accepted {target}"
        );
    }
    *plan.source_files.last_mut().ok_or("missing link")? = link("docs/link", "../src/lib.rs");
    plan.source_files.last_mut().ok_or("missing link")?.sha256 = "f".repeat(64);
    assert!(
        record_journal(
            &Registry::default(),
            &identity(),
            JournalStage::Started,
            &plan,
            None
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn published_archive_evidence_rejects_symlinks() -> Result<(), Box<dyn std::error::Error>> {
    let plan = receipt("0.1.1");
    let remote = Registry::default();
    record_journal(&remote, &identity(), JournalStage::Started, &plan, None)?;
    let mut result = completion("0.1.1");
    result.packages[0].files[0].kind = JournalFileKind::Symlink {
        target: "AGENTS.md".to_owned(),
    };
    assert!(
        record_journal(
            &remote,
            &identity(),
            JournalStage::Completed,
            &plan,
            Some(&result)
        )
        .is_err()
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn journal_source_binding_requires_exact_approved_tree() -> Result<(), Box<dyn std::error::Error>> {
    use std::{fs, os::unix::fs::symlink, process::Command};
    use velnor_actions_orchestrator::{journal_source_inventory, verify_journal_source};
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    let git = |args: &[&str]| -> Result<String, Box<dyn std::error::Error>> {
        let output = Command::new("/usr/bin/git")
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
    fs::write(root.join("AGENTS.md"), "approved")?;
    symlink("AGENTS.md", root.join("CLAUDE.md"))?;
    git(&["add", "."])?;
    git(&["-c", "commit.gpgsign=false", "commit", "-qm", "fixture"])?;
    let sha = git(&["rev-parse", "HEAD"])?;
    let mut plan = receipt("0.1.1");
    plan.identity.source_sha.clone_from(&sha);
    plan.source_files = journal_source_inventory(root, &sha, test_git())?;
    verify_journal_source(root, &plan, test_git())?;
    let mut journal = Registry::default();
    let mut forged = plan.clone();
    forged.source_files[0].sha256 = "f".repeat(64);
    assert!(verify_journal_source(root, &forged, test_git()).is_err());
    let mut wrong_identity = plan.identity.clone();
    wrong_identity.source_sha = "0".repeat(40);
    let mut wrong_source = plan.clone();
    wrong_source.identity = wrong_identity;
    assert!(verify_journal_source(root, &wrong_source, test_git()).is_err());
    seed_started_record(&journal, &plan.identity, &plan)?;
    assert_eq!(journal.refs.borrow().len(), 1);
    verify_prior_run_cases(&mut journal, &plan, root)?;
    verify_new_crate_prior_intention(&plan, root)?;
    let original = plan.clone();
    plan.source_files[0].sha256 = "f".repeat(64);
    assert!(verify_journal_source(root, &plan, test_git()).is_err());
    plan = original.clone();
    plan.source_files.pop();
    assert!(verify_journal_source(root, &plan, test_git()).is_err());
    plan = original.clone();
    plan.source_files.push(plan.source_files[0].clone());
    assert!(verify_journal_source(root, &plan, test_git()).is_err());
    plan = original.clone();
    plan.source_files[0].path = "forged".to_owned();
    assert!(verify_journal_source(root, &plan, test_git()).is_err());
    plan = original.clone();
    plan.source_files[0].kind = JournalFileKind::Regular { executable: true };
    assert!(verify_journal_source(root, &plan, test_git()).is_err());
    plan = original.clone();
    plan.source_files[1].kind = JournalFileKind::Symlink {
        target: "../escape".to_owned(),
    };
    assert!(verify_journal_source(root, &plan, test_git()).is_err());
    plan = original.clone();
    plan.source_files.reverse();
    assert!(verify_journal_source(root, &plan, test_git()).is_err());
    plan = original.clone();
    plan.identity.source_sha = "0".repeat(40);
    assert!(verify_journal_source(root, &plan, test_git()).is_err());
    git(&["rm", "--cached", "AGENTS.md"])?;
    assert!(verify_journal_source(root, &original, test_git()).is_err());
    Ok(())
}
