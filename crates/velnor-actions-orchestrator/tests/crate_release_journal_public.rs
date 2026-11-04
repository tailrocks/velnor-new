// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate as velnor_actions_orchestrator;
use sha1_checked::{Digest, Sha1};
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use velnor_actions_orchestrator::{
    OrchestratorError,
    crate_release_journal::{
        JournalAuthentication, JournalCompletion, JournalFile, JournalFileKind, JournalIdentity,
        JournalIo, JournalOperation, JournalOperationRefs, JournalOwner, JournalOwnerKind,
        JournalPackage, JournalPublishedPackage, JournalReceipt, JournalRegistry, JournalStage,
        JournalTag, JournalToolPins, inspect_journal, inspect_prior_history,
        inspect_prior_incomplete_evidence, record_journal, record_source_bound_start,
    },
};

#[derive(Default)]
struct Registry {
    refs: RefCell<BTreeMap<String, String>>,
    blobs: RefCell<BTreeMap<String, Vec<u8>>>,
    tags: RefCell<BTreeMap<String, JournalTag>>,
    reject_rules: bool,
    fail_ref_after_write: bool,
    fail_ref_before_write: Option<()>,
    wrong_blob_sha: bool,
    wrong_tag_target: Option<()>,
    wrong_tag_sha: Option<()>,
    access_checks: Cell<usize>,
    reject_access: Option<()>,
    inject_after_list: Cell<bool>,
    move_on_second_list: Cell<u8>,
}

impl JournalOperationRefs for Registry {
    fn list_operation_refs(
        &self,
        identity: &JournalIdentity,
    ) -> Result<Vec<(String, String)>, OrchestratorError> {
        if self.reject_access.is_some() {
            return Err(err());
        }
        let prefix = format!(
            "refs/tags/velnor-receipt/{}/{}/operation-",
            identity.run_id, identity.run_attempt
        );
        if self.move_on_second_list.get() == 1 {
            self.move_on_second_list.set(2);
        } else if self.move_on_second_list.get() == 2 {
            self.refs
                .borrow_mut()
                .insert(format!("{prefix}1"), "a".repeat(40));
            self.move_on_second_list.set(0);
        }
        let listed = self
            .refs
            .borrow()
            .iter()
            .filter(|(name, _)| name.starts_with(&prefix))
            .map(|(name, sha)| (name.clone(), sha.clone()))
            .collect();
        if self.inject_after_list.replace(false) {
            self.refs
                .borrow_mut()
                .insert(format!("{prefix}1"), "a".repeat(40));
        }
        Ok(listed)
    }
}

impl JournalIo for Registry {
    fn check_repository_access(&self, _: &str) -> Result<(), OrchestratorError> {
        self.access_checks.set(self.access_checks.get() + 1);
        if self.reject_access.is_some() {
            Err(err())
        } else {
            Ok(())
        }
    }
    fn check_tag_rules(&self, _: &str) -> Result<(), OrchestratorError> {
        if self.reject_rules {
            Err(err())
        } else {
            Ok(())
        }
    }
    fn get_ref(&self, _: &str, reference: &str) -> Result<Option<String>, OrchestratorError> {
        Ok(self.refs.borrow().get(reference).cloned())
    }
    fn create_blob(&self, _: &str, content: &[u8]) -> Result<String, OrchestratorError> {
        let mut hasher = Sha1::new();
        hasher.update(format!("blob {}\0", content.len()).as_bytes());
        hasher.update(content);
        let id = hasher
            .try_finalize()
            .hash()
            .iter()
            .fold(String::new(), |mut output, byte| {
                output.push(char::from(b"0123456789abcdef"[usize::from(byte >> 4)]));
                output.push(char::from(b"0123456789abcdef"[usize::from(byte & 15)]));
                output
            });
        self.blobs.borrow_mut().insert(id.clone(), content.to_vec());
        Ok(if self.wrong_blob_sha {
            "d".repeat(40)
        } else {
            id
        })
    }
    fn create_tag_object(&self, _: &str, tag: &JournalTag) -> Result<String, OrchestratorError> {
        let raw = raw_tag(tag);
        let mut hasher = Sha1::new();
        hasher.update(format!("tag {}\0", raw.len()).as_bytes());
        hasher.update(&raw);
        let id = if self.wrong_tag_sha.is_some() {
            "e".repeat(40)
        } else {
            hasher
                .try_finalize()
                .hash()
                .iter()
                .fold(String::new(), |mut output, byte| {
                    output.push(char::from(b"0123456789abcdef"[usize::from(byte >> 4)]));
                    output.push(char::from(b"0123456789abcdef"[usize::from(byte & 15)]));
                    output
                })
        };
        self.tags.borrow_mut().insert(id.clone(), tag.clone());
        Ok(id)
    }
    fn create_ref(&self, _: &str, reference: &str, tag_sha: &str) -> Result<(), OrchestratorError> {
        if self.fail_ref_before_write.is_some() {
            return Err(err());
        }
        if self.refs.borrow().contains_key(reference) {
            return Err(err());
        }
        self.refs
            .borrow_mut()
            .insert(reference.to_owned(), tag_sha.to_owned());
        if self.fail_ref_after_write {
            return Err(err());
        }
        Ok(())
    }
    fn get_tag_object(&self, _: &str, sha: &str) -> Result<JournalTag, OrchestratorError> {
        let mut tag = self.tags.borrow().get(sha).cloned().ok_or_else(err)?;
        if self.wrong_tag_target.is_some() {
            "commit".clone_into(&mut tag.object_type);
        }
        Ok(tag)
    }
    fn get_verified_tag_bytes(&self, _: &str, sha: &str) -> Result<Vec<u8>, OrchestratorError> {
        Ok(raw_tag(self.tags.borrow().get(sha).ok_or_else(err)?))
    }
    fn get_blob(&self, _: &str, sha: &str) -> Result<Vec<u8>, OrchestratorError> {
        self.blobs.borrow().get(sha).cloned().ok_or_else(err)
    }
}

#[test]
fn untrusted_object_id_or_tag_target_never_creates_a_ref() {
    for remote in [
        Registry {
            wrong_blob_sha: true,
            ..Registry::default()
        },
        Registry {
            wrong_tag_target: Some(()),
            ..Registry::default()
        },
    ] {
        assert!(
            record_journal(
                &remote,
                &identity(),
                JournalStage::Started,
                &receipt("0.1.1"),
                None
            )
            .is_err()
        );
        assert!(remote.refs.borrow().is_empty());
    }
    let mut bad = identity();
    bad.repository = "../repo".to_owned();
    assert!(
        record_journal(
            &Registry::default(),
            &bad,
            JournalStage::Started,
            &receipt("0.1.1"),
            None
        )
        .is_err()
    );
}

fn err() -> OrchestratorError {
    OrchestratorError::Contract {
        problem: "fake_remote_failure".to_owned(),
    }
}

fn identity() -> JournalIdentity {
    JournalIdentity {
        repository: "example/demo-crate".to_owned(),
        workflow_sha: "a".repeat(40),
        source_sha: "b".repeat(40),
        run_id: 42,
        run_attempt: 1,
    }
}

fn receipt(version: &str) -> JournalReceipt {
    JournalReceipt {
        semver_evidence: BTreeMap::from([(
            "demo-crate".into(),
            crate::crate_release_semver::SemverEvidence::NoBaseline {
                checker_sha256: "bc9eb06ce04e97d6160c875ffdee1f6e56793e423dea025f344826be03e66829"
                    .into(),
            },
        )]),
        predecessor: None,
        identity: identity(),
        intent_sha256: "c".repeat(64),
        base_intent_sha256: "c".repeat(64),
        registry: JournalRegistry::CratesIo,
        generator_version: "0.1.72".to_owned(),
        release_plz_version: "0.4.3".to_owned(),
        rust_version: "1.97.1".to_owned(),
        expected_tools: JournalToolPins {
            generator_sha256: "1".repeat(64),
            release_plz_sha256: "2".repeat(64),
            rust_manifest_sha256: "3".repeat(64),
        },
        authentication: JournalAuthentication::TrustedPublishing,
        required_check_id: 17,
        trusted_reviewer_id: 139_017,
        workflow_run_url: "https://github.com/example/demo-crate/actions/runs/42".to_owned(),
        source_files: vec![JournalFile {
            path: "src/lib.rs".to_owned(),
            sha256: "e".repeat(64),
            kind: JournalFileKind::Regular { executable: false },
        }],
        packages: vec![JournalPackage {
            name: "demo-crate".to_owned(),
            version: version.to_owned(),
            manifest: "Cargo.toml".to_owned(),
            tag: format!("demo-crate-v{version}"),
            owners: crate::crate_release_journal::JournalOwnership::Existing {
                owners: vec![JournalOwner {
                    id: 1,
                    login: "donbeave".to_owned(),
                    kind: JournalOwnerKind::User,
                }],
            },
        }],
    }
}

fn completion(version: &str) -> JournalCompletion {
    JournalCompletion {
        post_publication_file_hashes: JournalToolPins {
            generator_sha256: "1".repeat(64),
            release_plz_sha256: "2".repeat(64),
            rust_manifest_sha256: "3".repeat(64),
        },
        packages: vec![JournalPublishedPackage {
            name: "demo-crate".to_owned(),
            version: version.to_owned(),
            owners: vec![JournalOwner {
                id: 1,
                login: "donbeave".to_owned(),
                kind: JournalOwnerKind::User,
            }],
            registry_checksum: "d".repeat(64),
            downloaded_checksum: "d".repeat(64),
            tag_target: "b".repeat(40),
            release_id: 12,
            release_url: format!(
                "https://github.com/example/demo-crate/releases/tag/demo-crate-v{version}"
            ),
            files: vec![JournalFile {
                path: "src/lib.rs".to_owned(),
                sha256: "e".repeat(64),
                kind: JournalFileKind::Regular { executable: false },
            }],
            features: BTreeMap::new(),
            operations: vec![JournalOperation::Published],
        }],
    }
}

#[test]
fn completion_requires_observed_tool_pins_to_match_start() -> Result<(), Box<dyn std::error::Error>>
{
    let remote = Registry::default();
    let receipt = receipt("0.1.1");
    record_journal(&remote, &identity(), JournalStage::Started, &receipt, None)?;
    let mut result = completion("0.1.1");
    result.post_publication_file_hashes.release_plz_sha256 = "4".repeat(64);
    assert!(
        record_journal(
            &remote,
            &identity(),
            JournalStage::Completed,
            &receipt,
            Some(&result)
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn completion_requires_current_admitted_owners() -> Result<(), Box<dyn std::error::Error>> {
    let remote = Registry::default();
    let receipt = receipt("0.1.1");
    record_journal(&remote, &identity(), JournalStage::Started, &receipt, None)?;
    for owners in [
        Vec::new(),
        vec![JournalOwner {
            id: 2,
            login: "donbeave".into(),
            kind: JournalOwnerKind::User,
        }],
        vec![JournalOwner {
            id: 0,
            login: "donbeave".into(),
            kind: JournalOwnerKind::User,
        }],
        vec![
            JournalOwner {
                id: 1,
                login: "donbeave".into(),
                kind: JournalOwnerKind::User
            };
            2
        ],
    ] {
        let mut result = completion("0.1.1");
        result.packages[0].owners = owners;
        assert!(
            record_journal(
                &remote,
                &identity(),
                JournalStage::Completed,
                &receipt,
                Some(&result)
            )
            .is_err()
        );
        assert_eq!(remote.refs.borrow().len(), 1);
    }
    Ok(())
}

#[test]
fn start_then_complete_is_read_back_and_never_overwritten() -> Result<(), Box<dyn std::error::Error>>
{
    let remote = Registry::default();
    let receipt = receipt("0.1.1");
    let started = record_journal(&remote, &identity(), JournalStage::Started, &receipt, None)?;
    assert_eq!(started, "refs/tags/velnor-receipt/42/1/publication-started");
    assert_eq!(
        record_journal(&remote, &identity(), JournalStage::Started, &receipt, None)?,
        started
    );
    let result = completion("0.1.1");
    let completed = record_journal(
        &remote,
        &identity(),
        JournalStage::Completed,
        &receipt,
        Some(&result),
    )?;
    assert_eq!(completed, "refs/tags/velnor-receipt/42/1/reconciled");
    assert_eq!(remote.refs.borrow().len(), 2);
    assert_eq!(
        inspect_journal(&remote, &identity(), JournalStage::Completed)?
            .ok_or("missing")?
            .completion,
        Some(result)
    );
    Ok(())
}

#[test]
fn ambiguous_write_and_unprotected_tags_do_not_authorize_release() {
    let receipt = receipt("0.1.1");
    let failing = Registry {
        fail_ref_after_write: true,
        ..Registry::default()
    };
    assert!(
        matches!(record_journal(&failing, &identity(), JournalStage::Started, &receipt, None), Err(error) if error.to_string().contains("crate_release_journal_ref_write_ambiguous"))
    );
    assert_eq!(failing.refs.borrow().len(), 1);
    assert_eq!(
        inspect_journal(&failing, &identity(), JournalStage::Started)
            .ok()
            .flatten()
            .map(|e| e.receipt),
        Some(receipt.clone())
    );
    assert!(record_journal(&failing, &identity(), JournalStage::Started, &receipt, None).is_ok());
    let mut before = Registry {
        fail_ref_before_write: Some(()),
        ..Registry::default()
    };
    assert!(record_journal(&before, &identity(), JournalStage::Started, &receipt, None).is_err());
    assert!(before.refs.borrow().is_empty());
    before.fail_ref_before_write = None;
    assert!(record_journal(&before, &identity(), JournalStage::Started, &receipt, None).is_ok());
    before.refs.borrow_mut().insert(
        "refs/tags/velnor-receipt/42/1/publication-started".into(),
        "e".repeat(40),
    );
    assert!(record_journal(&before, &identity(), JournalStage::Started, &receipt, None).is_err());
    let unprotected = Registry {
        reject_rules: true,
        ..Registry::default()
    };
    assert!(
        record_journal(
            &unprotected,
            &identity(),
            JournalStage::Started,
            &receipt,
            None
        )
        .is_err()
    );
    assert!(unprotected.blobs.borrow().is_empty());
    assert!(
        record_journal(
            &Registry::default(),
            &identity(),
            JournalStage::Completed,
            &receipt,
            Some(&completion("0.1.1"))
        )
        .is_err()
    );
}

#[test]
fn inspection_rejects_cross_run_and_tampering() -> Result<(), Box<dyn std::error::Error>> {
    let remote = Registry::default();
    let receipt = receipt("0.1.1");
    assert!(inspect_journal(&remote, &identity(), JournalStage::Started)?.is_none());
    record_journal(&remote, &identity(), JournalStage::Started, &receipt, None)?;
    let mut other = identity();
    other.source_sha = "c".repeat(40);
    assert!(inspect_journal(&remote, &other, JournalStage::Started).is_err());
    let changed = self::receipt("0.1.2");
    assert!(
        record_journal(
            &remote,
            &identity(),
            JournalStage::Completed,
            &changed,
            Some(&completion("0.1.2"))
        )
        .is_err()
    );
    remote
        .blobs
        .borrow_mut()
        .values_mut()
        .next()
        .ok_or("blob")?
        .push(b' ');
    assert!(inspect_journal(&remote, &identity(), JournalStage::Started).is_err());
    Ok(())
}

#[test]
fn receipt_schema_rejects_unknown_fields_and_mismatched_results()
-> Result<(), Box<dyn std::error::Error>> {
    let receipt = receipt("0.1.1");
    let mut value = serde_json::to_value(&receipt)?;
    value.as_object_mut().ok_or("object")?.insert(
        "token".to_owned(),
        serde_json::Value::String("dummy".to_owned()),
    );
    assert!(serde_json::from_value::<JournalReceipt>(value).is_err());
    let remote = Registry::default();
    record_journal(&remote, &identity(), JournalStage::Started, &receipt, None)?;
    let mut result = completion("0.1.1");
    result.packages[0].downloaded_checksum = "f".repeat(64);
    assert!(
        record_journal(
            &remote,
            &identity(),
            JournalStage::Completed,
            &receipt,
            Some(&result)
        )
        .is_err()
    );
    assert_eq!(remote.refs.borrow().len(), 1);
    Ok(())
}

#[test]
fn semver_receipt_rejects_missing_forged_or_impossible_evidence() {
    use crate::crate_release_semver::SemverEvidence;
    let good = receipt("0.1.1");
    let mut bad = Vec::new();
    let mut missing = good.clone();
    missing.semver_evidence.clear();
    bad.push(missing);
    let mut unknown = good.clone();
    unknown.semver_evidence.insert(
        "other".into(),
        SemverEvidence::Deferred {
            predecessors: vec!["demo-crate".into()],
        },
    );
    bad.push(unknown);
    let mut digest = good.clone();
    digest.semver_evidence.insert(
        "demo-crate".into(),
        SemverEvidence::NoBaseline {
            checker_sha256: "a".repeat(64),
        },
    );
    bad.push(digest);
    let mut baseline = good.clone();
    baseline.semver_evidence.insert(
        "demo-crate".into(),
        SemverEvidence::Checked {
            baseline: "0.1.1".into(),
            baseline_checksum: "d".repeat(64),
            checker_sha256: "bc9eb06ce04e97d6160c875ffdee1f6e56793e423dea025f344826be03e66829"
                .into(),
        },
    );
    bad.push(baseline);
    let mut checksum = good.clone();
    checksum.semver_evidence.insert(
        "demo-crate".into(),
        SemverEvidence::Checked {
            baseline: "0.1.0".into(),
            baseline_checksum: "A".repeat(64),
            checker_sha256: "bc9eb06ce04e97d6160c875ffdee1f6e56793e423dea025f344826be03e66829"
                .into(),
        },
    );
    bad.push(checksum);
    let mut deferred = good;
    deferred.semver_evidence.insert(
        "demo-crate".into(),
        SemverEvidence::Deferred {
            predecessors: vec!["demo-crate".into()],
        },
    );
    bad.push(deferred);
    let remote = Registry::default();
    for invalid in bad {
        assert!(
            record_journal(&remote, &identity(), JournalStage::Started, &invalid, None).is_err()
        );
    }
    assert!(remote.refs.borrow().is_empty());
}

#[test]
fn tag_object_sha_must_match_exact_git_object_bytes() {
    let remote = Registry {
        wrong_tag_sha: Some(()),
        ..Registry::default()
    };
    assert!(
        record_journal(
            &remote,
            &identity(),
            JournalStage::Started,
            &receipt("0.1.1"),
            None
        )
        .is_err()
    );
    assert!(remote.refs.borrow().is_empty());
}

fn raw_tag(tag: &JournalTag) -> Vec<u8> {
    format!(
        "object {}\ntype {}\ntag {}\ntagger Velnor Release <release@velnor.invalid> 0 +0000\n\n{}",
        tag.object_sha, tag.object_type, tag.name, tag.message
    )
    .into_bytes()
}

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
    plan.source_files = journal_source_inventory(root, &sha)?;
    verify_journal_source(root, &plan)?;
    let mut journal = Registry::default();
    let mut forged = plan.clone();
    forged.source_files[0].sha256 = "f".repeat(64);
    assert!(record_source_bound_start(&journal, &plan.identity, &forged, root).is_err());
    assert!(journal.refs.borrow().is_empty());
    let mut wrong_identity = plan.identity.clone();
    wrong_identity.source_sha = "0".repeat(40);
    assert!(record_source_bound_start(&journal, &wrong_identity, &plan, root).is_err());
    assert!(journal.refs.borrow().is_empty());
    record_source_bound_start(&journal, &plan.identity, &plan, root)?;
    assert_eq!(journal.refs.borrow().len(), 1);
    verify_prior_run_cases(&mut journal, &plan, root)?;
    verify_new_crate_prior_intention(&plan, root)?;
    let original = plan.clone();
    plan.source_files[0].sha256 = "f".repeat(64);
    assert!(verify_journal_source(root, &plan).is_err());
    plan = original.clone();
    plan.source_files.pop();
    assert!(verify_journal_source(root, &plan).is_err());
    plan = original.clone();
    plan.source_files.push(plan.source_files[0].clone());
    assert!(verify_journal_source(root, &plan).is_err());
    plan = original.clone();
    plan.source_files[0].path = "forged".to_owned();
    assert!(verify_journal_source(root, &plan).is_err());
    plan = original.clone();
    plan.source_files[0].kind = JournalFileKind::Regular { executable: true };
    assert!(verify_journal_source(root, &plan).is_err());
    plan = original.clone();
    plan.source_files[1].kind = JournalFileKind::Symlink {
        target: "../escape".to_owned(),
    };
    assert!(verify_journal_source(root, &plan).is_err());
    plan = original.clone();
    plan.source_files.reverse();
    assert!(verify_journal_source(root, &plan).is_err());
    plan = original.clone();
    plan.identity.source_sha = "0".repeat(40);
    assert!(verify_journal_source(root, &plan).is_err());
    git(&["rm", "--cached", "AGENTS.md"])?;
    assert!(verify_journal_source(root, &original).is_err());
    Ok(())
}

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
                "ubuntu-24.04",
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
                "ubuntu-24.04",
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
    assert!(super::inspect_current_history(&remote, &identity(), &plan).is_err());
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
        super::inspect_current_history(&remote, &identity(), &plan)?.len(),
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
                "ubuntu-24.04",
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
                "ubuntu-24.04",
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

fn verify_release_result_shape(
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

fn verify_chain_and_rerun(
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
    record_source_bound_start(&chained_journal, &older.identity, &older, root)?;
    let mut chained = plan.clone();
    chained.predecessor = Some(older.identity.clone());
    record_source_bound_start(&chained_journal, &chained.identity, &chained, root)?;
    assert_eq!(
        super::inspect_prior_chain(&chained_journal, current, root)?.len(),
        2
    );
    let cyclic_journal = Registry::default();
    older.predecessor = Some(chained.identity.clone());
    record_source_bound_start(&cyclic_journal, &older.identity, &older, root)?;
    record_source_bound_start(&cyclic_journal, &chained.identity, &chained, root)?;
    assert!(super::inspect_prior_chain(&cyclic_journal, current, root).is_err());
    let mut rerun = current.clone();
    rerun.identity.run_id = plan.identity.run_id;
    rerun.identity.run_attempt = plan.identity.run_attempt + 1;
    rerun.workflow_run_url = plan.workflow_run_url.clone();
    assert_eq!(
        inspect_prior_incomplete_evidence(journal, &plan.identity, &rerun, root)?.receipt,
        *plan
    );
    rerun.identity.run_attempt = plan.identity.run_attempt;
    assert!(inspect_prior_incomplete_evidence(journal, &plan.identity, &rerun, root).is_err());
    Ok(())
}

fn verify_prior_run_cases(
    journal: &mut Registry,
    plan: &JournalReceipt,
    root: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut current = plan.clone();
    current.identity.run_id = 43;
    current.predecessor = Some(plan.identity.clone());
    current.identity.workflow_sha = "c".repeat(40);
    current.workflow_run_url = "https://github.com/example/demo-crate/actions/runs/43".into();
    let prior = inspect_prior_incomplete_evidence(journal, &plan.identity, &current, root)?;
    assert_eq!(&prior.receipt, plan);
    verify_chain_and_rerun(journal, plan, &current, root)?;
    let history = inspect_prior_history(journal, &plan.identity, &current, root)?;
    assert_eq!(history.mutations.len(), 0);
    journal.inject_after_list.set(true);
    assert!(inspect_prior_history(journal, &plan.identity, &current, root).is_err());
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
    assert!(inspect_prior_history(journal, &plan.identity, &current, root).is_err());
    journal.refs.borrow_mut().remove(&gap);
    let malformed = format!(
        "refs/tags/velnor-receipt/{}/{}/operation-01",
        plan.identity.run_id, plan.identity.run_attempt
    );
    journal
        .refs
        .borrow_mut()
        .insert(malformed.clone(), "a".repeat(40));
    assert!(inspect_prior_history(journal, &plan.identity, &current, root).is_err());
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
        assert!(inspect_prior_incomplete_evidence(journal, &plan.identity, &forged, root).is_err());
    }
    assert_eq!(journal.refs.borrow().len(), 1);
    journal.reject_access = Some(());
    assert!(inspect_prior_incomplete_evidence(journal, &plan.identity, &current, root).is_err());
    journal.reject_access = None;
    let mut missing = plan.identity.clone();
    missing.run_id = 44;
    assert!(inspect_prior_incomplete_evidence(journal, &missing, &current, root).is_err());
    verify_prior_operation_chain(journal, plan, &current, root)?;
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
    assert!(inspect_prior_incomplete_evidence(journal, &plan.identity, &current, root).is_err());
    Ok(())
}

fn verify_prior_operation_chain(
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
                "ubuntu-24.04",
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
    let history = inspect_prior_history(journal, &plan.identity, current, root)?;
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
    let history = inspect_prior_history(journal, &plan.identity, current, root)?;
    assert_eq!(history.mutations.len(), 2);
    assert_eq!(history.mutations[1], mutation);
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

fn journal_existing_owners(
    value: &crate::crate_release_journal::JournalOwnership,
) -> Vec<crate::crate_release_journal::JournalOwner> {
    let crate::crate_release_journal::JournalOwnership::Existing { owners } = value else {
        panic!("expected existing owners")
    };
    owners.clone()
}

fn verify_new_crate_prior_intention(
    plan: &JournalReceipt,
    root: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::crate_release_journal::{
        JournalIntendedMutation, JournalMutationKind, JournalOwnership, record_journal_intent,
        record_source_bound_start,
    };
    use crate::crate_release_semver::SemverEvidence;
    let journal = Registry::default();
    let mut prior = plan.clone();
    prior.authentication = JournalAuthentication::BootstrapToken;
    prior.packages[0].owners = JournalOwnership::NewCrate;
    record_source_bound_start(&journal, &prior.identity, &prior, root)?;
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
    assert!(inspect_prior_history(&journal, &prior.identity, &current, root).is_err());
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
    assert!(inspect_prior_history(&journal, &prior.identity, &current, root).is_ok());
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
    assert!(inspect_prior_history(&journal, &prior.identity, &current, root).is_ok());
    reject_changed_new_crate_result(&journal, &prior, &current, root)?;
    Ok(())
}

fn reject_moved_history_ref(
    journal: &Registry,
    prior: &JournalReceipt,
    current: &JournalReceipt,
    root: &std::path::Path,
) {
    let original_refs = journal.refs.borrow().clone();
    let object_counts = (journal.tags.borrow().len(), journal.blobs.borrow().len());
    journal.move_on_second_list.set(1);
    assert!(
        inspect_prior_history(journal, &prior.identity, current, root).is_err(),
        "moving a journal ref between history observations must be rejected"
    );
    assert_eq!(
        (journal.tags.borrow().len(), journal.blobs.borrow().len()),
        object_counts
    );
    *journal.refs.borrow_mut() = original_refs;
}

fn reject_changed_new_crate_result(
    journal: &Registry,
    prior: &JournalReceipt,
    current: &JournalReceipt,
    root: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::crate_release_journal::JournalOwnership;
    use crate::crate_release_semver::SemverEvidence;
    let mut wrong_checksum = current.clone();
    wrong_checksum.semver_evidence.insert(
        "demo-crate".into(),
        SemverEvidence::PriorNoBaselinePending {
            checker_sha256: "bc9eb06ce04e97d6160c875ffdee1f6e56793e423dea025f344826be03e66829"
                .into(),
            target_checksum: "e".repeat(64),
        },
    );
    assert!(inspect_prior_history(journal, &prior.identity, &wrong_checksum, root).is_err());
    let mut wrong_owner = current.clone();
    let JournalOwnership::Existing { owners } = &mut wrong_owner.packages[0].owners else {
        return Err("owner".into());
    };
    owners[0].id += 1;
    assert!(inspect_prior_history(journal, &prior.identity, &wrong_owner, root).is_err());
    let mut bad = current.clone();
    bad.identity.source_sha = "f".repeat(40);
    assert!(inspect_prior_history(journal, &prior.identity, &bad, root).is_err());
    Ok(())
}

fn reject_forged_new_crate_prior(
    plan: &JournalReceipt,
    prior: &JournalReceipt,
    current: &JournalReceipt,
    root: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::crate_release_journal::{
        JournalIntendedMutation, JournalMutationKind, record_journal_intent,
        record_source_bound_start,
    };
    use crate::crate_release_semver::SemverEvidence;
    let forged_journal = Registry::default();
    let mut existing_prior = prior.clone();
    existing_prior.packages[0].owners = plan.packages[0].owners.clone();
    record_source_bound_start(
        &forged_journal,
        &existing_prior.identity,
        &existing_prior,
        root,
    )?;
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
        inspect_prior_history(&forged_journal, &existing_prior.identity, current, root).is_err()
    );
    Ok(())
}
