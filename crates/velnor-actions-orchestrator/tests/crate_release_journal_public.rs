// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

fn test_git() -> &'static crate::crate_release_git::ReleaseGit {
    static GIT: std::sync::OnceLock<crate::crate_release_git::ReleaseGit> =
        std::sync::OnceLock::new();
    GIT.get_or_init(crate::crate_release_git::ReleaseGit::host_fixture)
}

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
        JournalTag, JournalToolPins, fixture_storage::seed_started_record, inspect_journal,
        inspect_prior_history, inspect_prior_incomplete_evidence, record_journal,
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
    change_source_after_list: RefCell<Option<std::path::PathBuf>>,
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
        if let Some(path) = self.change_source_after_list.borrow_mut().take() {
            std::fs::write(path, "changed during journal read").map_err(|_| err())?;
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
            crate::release::crate_release_semver::SemverEvidence::NoBaseline {
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
            tag_target: None,
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

fn raw_tag(tag: &JournalTag) -> Vec<u8> {
    format!(
        "object {}\ntype {}\ntag {}\ntagger Velnor Release <release@velnor.invalid> 0 +0000\n\n{}",
        tag.object_sha, tag.object_type, tag.name, tag.message
    )
    .into_bytes()
}

#[path = "crate_release_journal_public/chain_support.rs"]
mod chain_support;
#[path = "crate_release_journal_public/completion.rs"]
mod completion;
#[path = "crate_release_journal_public/evidence.rs"]
mod evidence;
#[path = "crate_release_journal_public/historical_schema.rs"]
mod historical_schema;
#[path = "crate_release_journal_public/mutation.rs"]
mod mutation;
#[path = "crate_release_journal_public/owner_identity.rs"]
mod owner_identity;
#[path = "crate_release_journal_public/ownership_support.rs"]
mod ownership_support;
use chain_support::*;
use ownership_support::*;
