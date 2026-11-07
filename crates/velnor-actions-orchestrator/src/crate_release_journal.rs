// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::{OrchestratorError, internal::internal};
use serde::{Deserialize, Serialize};
use sha1_checked::{Digest, Sha1};

#[path = "crate_release_journal_schema.rs"]
mod schema;
pub(crate) use schema::valid_source_files;
pub use schema::{
    JournalAuthentication, JournalCompletion, JournalFile, JournalFileKind, JournalOperation,
    JournalOwner, JournalOwnerKind, JournalOwnership, JournalPackage, JournalPublishedPackage,
    JournalReceipt, JournalRegistry, JournalToolPins,
};
use schema::{valid_completion, valid_receipt};

pub(crate) fn release_base_intent_sha256(
    intent: &velnor_actions_contract::config::CrateReleaseIntent,
) -> Result<String, OrchestratorError> {
    let mut base = intent.clone();
    base.journal_predecessor = None;
    base.continuation_source_sha = None;
    base.recovery = None;
    let bytes = velnor_actions_contract::canonical_json_bytes(&base).map_err(|_| invalid())?;
    Ok(crate::planner::generator::sha256_hex(&bytes))
}

const MAX_RECORD_BYTES: usize = 32 * 1024 * 1024;
const TAG_MESSAGE: &str = "velnor-release-receipt-v1";

#[path = "crate_release_journal_capacity.rs"]
mod capacity;
pub(crate) use capacity::{CapacityReservation, verify_sequence_capacity};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JournalIdentity {
    pub repository: String,
    pub workflow_sha: String,
    pub source_sha: String,
    pub run_id: u64,
    pub run_attempt: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum JournalStage {
    Started,
    Completed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JournalTag {
    pub name: String,
    pub object_type: String,
    pub object_sha: String,
    pub message: String,
}

pub trait JournalIo {
    fn check_repository_access(&self, repository: &str) -> Result<(), OrchestratorError>;
    fn check_tag_rules(&self, repository: &str) -> Result<(), OrchestratorError>;
    fn get_ref(
        &self,
        repository: &str,
        reference: &str,
    ) -> Result<Option<String>, OrchestratorError>;
    fn read_ref_with_access(
        &self,
        repository: &str,
        reference: &str,
    ) -> Result<Option<String>, OrchestratorError> {
        self.check_repository_access(repository)?;
        let observed = self.get_ref(repository, reference)?;
        if observed.is_none() {
            self.check_repository_access(repository)?;
        }
        Ok(observed)
    }
    fn create_blob(&self, repository: &str, content: &[u8]) -> Result<String, OrchestratorError>;
    fn create_tag_object(
        &self,
        repository: &str,
        tag: &JournalTag,
    ) -> Result<String, OrchestratorError>;
    fn create_ref(
        &self,
        repository: &str,
        reference: &str,
        tag_sha: &str,
    ) -> Result<(), OrchestratorError>;
    fn get_tag_object(&self, repository: &str, sha: &str) -> Result<JournalTag, OrchestratorError>;
    fn get_verified_tag_bytes(
        &self,
        repository: &str,
        sha: &str,
    ) -> Result<Vec<u8>, OrchestratorError>;
    fn get_blob(&self, repository: &str, sha: &str) -> Result<Vec<u8>, OrchestratorError>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct JournalRecord {
    schema: u8,
    identity: JournalIdentity,
    stage: JournalStage,
    receipt: JournalReceipt,
    completion: Option<JournalCompletion>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum JournalMutationKind {
    Upload,
    CreateTag,
    CreateRelease,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "phase", rename_all = "kebab-case", deny_unknown_fields)]
pub enum JournalMutationPhase {
    Intended,
    Observed { result: JournalMutationResult },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum JournalMutationResult {
    Upload {
        registry_checksum: String,
        downloaded_checksum: String,
        owners: Vec<JournalOwner>,
        features: std::collections::BTreeMap<String, Vec<String>>,
        files: Vec<JournalFile>,
    },
    Tag {
        name: String,
        target_sha: String,
    },
    Release {
        id: u64,
        tag: String,
        url: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JournalMutation {
    pub sequence: u32,
    pub package: String,
    pub version: String,
    pub kind: JournalMutationKind,
    pub semver_evidence: Option<crate::release::crate_release_semver::SemverEvidence>,
    pub phase: JournalMutationPhase,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MutationRecord {
    schema: u8,
    identity: JournalIdentity,
    receipt: JournalReceipt,
    mutation: JournalMutation,
}

#[cfg(test)]
pub(crate) fn fixture_mutation_record_bytes(
    receipt: &JournalReceipt,
    mutation: JournalMutation,
) -> Result<Vec<u8>, OrchestratorError> {
    velnor_actions_contract::canonical_json_bytes(&MutationRecord {
        schema: 1,
        identity: receipt.identity.clone(),
        receipt: receipt.clone(),
        mutation,
    })
    .map_err(|_| invalid())
}

#[cfg(test)]
pub(crate) fn fixture_stage_record_bytes(
    receipt: &JournalReceipt,
    completion: Option<JournalCompletion>,
) -> Result<Vec<u8>, OrchestratorError> {
    velnor_actions_contract::canonical_json_bytes(&JournalRecord {
        schema: 1,
        identity: receipt.identity.clone(),
        stage: if completion.is_some() {
            JournalStage::Completed
        } else {
            JournalStage::Started
        },
        receipt: receipt.clone(),
        completion,
    })
    .map_err(|_| invalid())
}

#[path = "crate_release_journal_history.rs"]
mod history;
#[path = "crate_release_journal_history_snapshot.rs"]
mod history_snapshot;
pub(crate) use history_snapshot::PriorHistorySnapshot;
#[path = "crate_release_journal_mutation.rs"]
mod mutation;
#[path = "crate_release_journal_records.rs"]
mod records;
#[cfg(test)]
pub(crate) use history::inspect_prior_chain;
pub use history::{
    JournalOperationRefs, PriorHistoryEvidence, inspect_prior_history,
    inspect_prior_incomplete_evidence,
};
pub(crate) use history::{
    PriorTransition, inspect_current_history, inspect_recoverable_history, walk_prior_history_until,
};
#[cfg(test)]
use mutation::record_journal_mutation;
pub use mutation::{JournalIntendedMutation, inspect_journal_mutation};
use mutation::{ensure_no_pending_mutation, read_mutation_at, valid_mutation};
pub(crate) use mutation::{
    mutation_name, record_journal_intent, record_verified_observation, valid_journal_phase,
};
pub(crate) use records::record_frozen_completion;
pub(crate) use records::record_validated_start;
pub use records::{JournalEvidence, inspect_journal};
use records::{create_content_addressed_tag, journal_name, read_record};

fn validate_identity(identity: &JournalIdentity) -> Result<(), OrchestratorError> {
    let parts: Vec<_> = identity.repository.split('/').collect();
    if parts.len() != 2
        || parts.iter().any(|part| {
            part.is_empty()
                || part.len() > 100
                || part.starts_with('.')
                || part.ends_with('.')
                || part.contains("..")
                || !part
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
        })
        || !valid_sha(&identity.workflow_sha)
        || !valid_sha(&identity.source_sha)
        || identity.run_id == 0
        || identity.run_attempt == 0
    {
        return Err(invalid());
    }
    Ok(())
}

fn valid_sha(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn invalid() -> OrchestratorError {
    internal("crate_release_journal_invalid")
}

fn verify_raw_tag<I: JournalIo>(
    io: &I,
    repository: &str,
    sha: &str,
    tag: &JournalTag,
) -> Result<(), OrchestratorError> {
    let raw = io
        .get_verified_tag_bytes(repository, sha)
        .map_err(|_| invalid())?;
    let expected = format!(
        "object {}\ntype {}\ntag {}\ntagger Velnor Release <release@velnor.invalid> 0 +0000\n\n{}",
        tag.object_sha, tag.object_type, tag.name, tag.message
    );
    if raw.len() > 4096 || raw != expected.as_bytes() {
        return Err(invalid());
    }
    let mut digest = Sha1::new();
    digest.update(format!("tag {}\0", raw.len()).as_bytes());
    digest.update(&raw);
    let bytes = digest.try_finalize();
    if bytes.has_collision() {
        return Err(invalid());
    }
    let computed = bytes.hash().iter().fold(String::new(), |mut output, byte| {
        output.push(char::from(b"0123456789abcdef"[usize::from(byte >> 4)]));
        output.push(char::from(b"0123456789abcdef"[usize::from(byte & 15)]));
        output
    });
    if computed != sha {
        return Err(invalid());
    }
    Ok(())
}

fn read_ref<I: JournalIo>(
    io: &I,
    repository: &str,
    reference: &str,
) -> Result<Option<String>, OrchestratorError> {
    io.read_ref_with_access(repository, reference)
        .map_err(|_| invalid())
}

#[cfg(test)]
#[path = "../tests/crate_release_journal_public.rs"]
mod tests;

#[cfg(test)]
#[test]
fn qualification_policy_change_changes_base_intent() -> Result<(), Box<dyn std::error::Error>> {
    let current: velnor_actions_contract::config::CrateReleaseIntent = serde_json::from_value(
        serde_json::json!({
            "schema":1,"repository":"owner/project","default_branch":"main","environment":"release",
            "required_workflow_path":".github/workflows/ci.yml","trusted_reviewer_id":139_017,
            "generator_version":"0.1.72","generator_source_sha":"c".repeat(40),"generator_manifest_sha256":"d".repeat(64),
            "release_plz_version":"0.4.3","runner":"ubuntu-26.04","authentication":"trusted-publishing",
            "rust_version":"1.97.1","manifest_path":"Cargo.toml","pr_write_paths":["Cargo.toml"],
            "registry":"crates-io","packages":["demo"],"publish_features":{"demo":[]},
            "tag_template":"{{ package }}-v{{ version }}"
        }),
    )?;
    let mut changed = current.clone();
    changed.oidc_qualification = true;
    assert_ne!(
        release_base_intent_sha256(&current)?,
        release_base_intent_sha256(&changed)?
    );
    Ok(())
}

#[cfg(test)]
pub(crate) fn fixture_record_journal_mutation<I: JournalIo>(
    io: &I,
    identity: &JournalIdentity,
    receipt: &JournalReceipt,
    mutation: &JournalMutation,
) -> Result<String, OrchestratorError> {
    record_journal_mutation(io, identity, receipt, mutation)
}

#[cfg(test)]
use records::record_journal;

#[cfg(test)]
pub(crate) use records::record_unprivileged_completion;

#[cfg(test)]
#[path = "../tests/support/journal_storage.rs"]
pub(crate) mod fixture_storage;
