// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    Cell, OrchestratorError, RefCell, RefCreatedHook, fixture_archive, fixture_journal_plan,
    fixture_no_baseline, make_fixture, prepare_crate_release, reject_observed_approval_revocation,
    reject_tampered_source, reject_wrong_authentication, test_git, velnor_actions_orchestrator,
    verify_metadata_observations, verify_upload_result,
};

#[test]
pub(super) fn github_release_evidence_requires_a_positive_numeric_id()
-> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let (_, _, github, _) = make_fixture(temp.path())?;
    github.tag_exists.set(true);
    assert!(
        velnor_actions_orchestrator::crate_release_github::read_github_release_evidence(
            "example/demo",
            "demo-v0.1.1",
            &github
        )?
        .is_some()
    );
    for variant in [5, 6, 7, 8, 9] {
        github.release_variant.set(variant);
        assert!(
            velnor_actions_orchestrator::crate_release_github::read_github_release_evidence(
                "example/demo",
                "demo-v0.1.1",
                &github
            )
            .is_err()
        );
    }
    Ok(())
}
use sha1_checked::{Digest as Sha1Digest, Sha1};
use std::collections::BTreeMap;
use velnor_actions_orchestrator::crate_release_journal::{JournalIo, JournalTag};
#[derive(Default)]
pub(super) struct JournalRegistryFake {
    pub(super) refs: RefCell<BTreeMap<String, String>>,
    pub(super) blobs: RefCell<BTreeMap<String, Vec<u8>>>,
    pub(super) tags: RefCell<BTreeMap<String, JournalTag>>,
    pub(super) reject_rules: bool,
    pub(super) fail_ref_after_write: bool,
    pub(super) fail_ref_before_write: Option<()>,
    pub(super) wrong_blob_sha: bool,
    pub(super) wrong_tag_target: Option<()>,
    pub(super) wrong_tag_sha: Option<()>,
    pub(super) access_checks: Cell<usize>,
    pub(super) write_calls: Cell<usize>,
    pub(super) reject_access: Option<()>,
    pub(super) after_ref_created: RefCell<Option<RefCreatedHook>>,
}

impl velnor_actions_orchestrator::crate_release_journal::JournalOperationRefs
    for JournalRegistryFake
{
    fn list_operation_refs(
        &self,
        identity: &velnor_actions_orchestrator::crate_release_journal::JournalIdentity,
    ) -> Result<Vec<(String, String)>, OrchestratorError> {
        let prefix = format!(
            "refs/tags/velnor-receipt/{}/{}/operation-",
            identity.run_id, identity.run_attempt
        );
        Ok(self
            .refs
            .borrow()
            .iter()
            .filter(|(name, _)| name.starts_with(&prefix))
            .map(|(name, sha)| (name.clone(), sha.clone()))
            .collect())
    }
}

impl JournalIo for JournalRegistryFake {
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
        self.write_calls.set(self.write_calls.get() + 1);
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
        self.write_calls.set(self.write_calls.get() + 1);
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
        self.write_calls.set(self.write_calls.get() + 1);
        if self.fail_ref_before_write.is_some() {
            return Err(err());
        }
        if self.refs.borrow().contains_key(reference) {
            return Err(err());
        }
        self.refs
            .borrow_mut()
            .insert(reference.to_owned(), tag_sha.to_owned());
        if let Some(hook) = self.after_ref_created.borrow().as_ref() {
            hook(reference);
        }
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

pub(super) fn raw_tag(tag: &JournalTag) -> Vec<u8> {
    format!(
        "object {}\ntype {}\ntag {}\ntagger Velnor Release <release@velnor.invalid> 0 +0000\n\n{}",
        tag.object_sha, tag.object_type, tag.name, tag.message
    )
    .into_bytes()
}

pub(super) fn err() -> OrchestratorError {
    OrchestratorError::Contract {
        problem: "fake_remote_failure".into(),
    }
}

#[test]
pub(super) fn observed_upload_is_derived_from_current_registry()
-> Result<(), Box<dyn std::error::Error>> {
    use velnor_actions_orchestrator::crate_release_controller::{
        CrateReleaseObservation, record_verified_mutation,
    };
    use velnor_actions_orchestrator::crate_release_journal::{
        JournalIntendedMutation, JournalMutationKind, inspect_journal_mutation,
        record_journal_intent, record_source_bound_start,
    };
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    let (intent, request, github, registry) = make_fixture(root)?;
    github.approved.set(true);
    let admission = prepare_crate_release(&intent, &request, &github, &registry, root)?;
    let (identity, plan) = fixture_journal_plan(root, &intent, &request, &admission)?;
    let journal = JournalRegistryFake::default();
    record_source_bound_start(&journal, &identity, &plan, root, test_git())?;
    record_journal_intent(
        &journal,
        &identity,
        &plan,
        &JournalIntendedMutation {
            semver_evidence: Some(fixture_no_baseline()),
            sequence: 1,
            package: "demo".into(),
            version: "0.1.1".into(),
            kind: JournalMutationKind::Upload,
        },
    )?;
    *registry.archive.borrow_mut() = fixture_archive(root)?;
    registry.absent.set(false);
    let obs = CrateReleaseObservation {
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
        sequence: 2,
    };
    let mut wrong_identity = identity.clone();
    wrong_identity.workflow_sha = "c".repeat(40);
    let wrong = CrateReleaseObservation {
        identity: &wrong_identity,
        ..obs
    };
    assert!(record_verified_mutation(&wrong).is_err());
    reject_wrong_authentication(&obs);
    let mut wrong_intent = intent.clone();
    wrong_intent.rust_version = if wrong_intent.rust_version == "1.97.1" {
        "1.99.1"
    } else {
        "1.97.1"
    }
    .into();
    let wrong = CrateReleaseObservation {
        intent: &wrong_intent,
        ..obs
    };
    assert!(record_verified_mutation(&wrong).is_err());
    let mut wrong_request = request.clone();
    wrong_request.source_sha = "d".repeat(40);
    let wrong = CrateReleaseObservation {
        request: &wrong_request,
        ..obs
    };
    assert!(record_verified_mutation(&wrong).is_err());
    reject_observed_approval_revocation(&obs)?;
    reject_tampered_source(&obs)?;
    assert!(inspect_journal_mutation(&journal, &identity, 2)?.is_none());
    github.approved.set(false);
    assert!(record_verified_mutation(&obs).is_err());
    assert!(inspect_journal_mutation(&journal, &identity, 2)?.is_none());
    github.approved.set(true);
    registry.archive.borrow_mut().push(0);
    assert!(record_verified_mutation(&obs).is_err());
    assert!(inspect_journal_mutation(&journal, &identity, 2)?.is_none());
    registry.archive.borrow_mut().pop();
    record_verified_mutation(&obs)?;
    verify_upload_result(&obs)?;
    verify_metadata_observations(&obs, &github)?;
    Ok(())
}
