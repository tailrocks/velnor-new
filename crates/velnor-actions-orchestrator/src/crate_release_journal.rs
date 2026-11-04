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
    base.recovery = None;
    let bytes = velnor_actions_contract::canonical_json_bytes(&base).map_err(|_| invalid())?;
    Ok(crate::generator::sha256_hex(&bytes))
}

const MAX_RECORD_BYTES: usize = 32 * 1024 * 1024;
const TAG_MESSAGE: &str = "velnor-release-receipt-v1";

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
    pub semver_evidence: Option<crate::crate_release_semver::SemverEvidence>,
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

pub fn inspect_journal_mutation<I: JournalIo>(
    io: &I,
    identity: &JournalIdentity,
    sequence: u32,
) -> Result<Option<JournalMutation>, OrchestratorError> {
    validate_identity(identity)?;
    if sequence == 0 || sequence > 1000 {
        return Err(invalid());
    }
    io.check_tag_rules(&identity.repository)
        .map_err(|_| invalid())?;
    let started = read_record(io, identity, JournalStage::Started)?;
    let mut previous = None;
    for current in 1..=sequence {
        let Some(record) = read_mutation(io, identity, current)? else {
            return if current == sequence {
                Ok(None)
            } else {
                Err(invalid())
            };
        };
        if record.receipt != started.receipt
            || !valid_mutation(&record.mutation, &started.receipt, previous.as_ref())
        {
            return Err(invalid());
        }
        previous = Some(record.mutation);
    }
    Ok(previous)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JournalIntendedMutation {
    pub sequence: u32,
    pub package: String,
    pub version: String,
    pub kind: JournalMutationKind,
    pub semver_evidence: Option<crate::crate_release_semver::SemverEvidence>,
}

pub(crate) fn record_journal_intent<I: JournalIo>(
    io: &I,
    identity: &JournalIdentity,
    receipt: &JournalReceipt,
    intended: &JournalIntendedMutation,
) -> Result<String, OrchestratorError> {
    let mutation = JournalMutation {
        sequence: intended.sequence,
        package: intended.package.clone(),
        version: intended.version.clone(),
        kind: intended.kind,
        semver_evidence: intended.semver_evidence.clone(),
        phase: JournalMutationPhase::Intended,
    };
    record_journal_mutation(io, identity, receipt, &mutation)
}

pub(crate) fn record_verified_observation<I: JournalIo>(
    io: &I,
    identity: &JournalIdentity,
    receipt: &JournalReceipt,
    observed: crate::crate_release_controller::VerifiedMutation,
) -> Result<String, OrchestratorError> {
    record_journal_mutation(io, identity, receipt, &observed.into_inner())
}

fn record_journal_mutation<I: JournalIo>(
    io: &I,
    identity: &JournalIdentity,
    receipt: &JournalReceipt,
    mutation: &JournalMutation,
) -> Result<String, OrchestratorError> {
    validate_identity(identity)?;
    if mutation.sequence == 0 || mutation.sequence > 1000 {
        return Err(invalid());
    }
    let started = read_record(io, identity, JournalStage::Started)?;
    if started.receipt != *receipt {
        return Err(invalid());
    }
    let previous = if mutation.sequence == 1 {
        None
    } else {
        Some(inspect_journal_mutation(io, identity, mutation.sequence - 1)?.ok_or_else(invalid)?)
    };
    if !valid_mutation(mutation, receipt, previous.as_ref()) {
        return Err(invalid());
    }
    let name = mutation_name(identity, mutation.sequence);
    io.check_tag_rules(&identity.repository)
        .map_err(|_| invalid())?;
    if read_ref(
        io,
        &identity.repository,
        &journal_name(identity, JournalStage::Completed),
    )
    .map_err(|_| invalid())?
    .is_some()
    {
        return Err(invalid());
    }
    if read_ref(io, &identity.repository, &name)
        .map_err(|_| invalid())?
        .is_some()
    {
        return if inspect_journal_mutation(io, identity, mutation.sequence)?
            == Some(mutation.clone())
        {
            Ok(name)
        } else {
            Err(invalid())
        };
    }
    let record = MutationRecord {
        schema: 1,
        identity: identity.clone(),
        receipt: receipt.clone(),
        mutation: mutation.clone(),
    };
    let bytes = velnor_actions_contract::canonical_json_bytes(&record).map_err(|_| invalid())?;
    if bytes.len() > MAX_RECORD_BYTES {
        return Err(invalid());
    }
    let tag = create_content_addressed_tag(io, identity, &name, &bytes)?;
    io.check_tag_rules(&identity.repository)
        .map_err(|_| invalid())?;
    if io.create_ref(&identity.repository, &name, &tag).is_err() {
        return Err(internal("crate_release_journal_ref_write_ambiguous"));
    }
    if read_ref(io, &identity.repository, &name)
        .map_err(|_| invalid())?
        .as_deref()
        != Some(tag.as_str())
        || inspect_journal_mutation(io, identity, mutation.sequence)? != Some(mutation.clone())
    {
        return Err(invalid());
    }
    io.check_tag_rules(&identity.repository)
        .map_err(|_| invalid())?;
    Ok(name)
}

fn ensure_no_pending_mutation<I: JournalIo>(
    io: &I,
    identity: &JournalIdentity,
    receipt: &JournalReceipt,
) -> Result<(), OrchestratorError> {
    let mut previous: Option<JournalMutation> = None;
    for sequence in 1..=1000 {
        let Some(record) = read_mutation(io, identity, sequence)? else {
            return match previous {
                None
                | Some(JournalMutation {
                    phase: JournalMutationPhase::Observed { .. },
                    ..
                }) => Ok(()),
                _ => Err(invalid()),
            };
        };
        if &record.receipt != receipt
            || !valid_mutation(&record.mutation, receipt, previous.as_ref())
        {
            return Err(invalid());
        }
        previous = Some(record.mutation);
    }
    Err(invalid())
}

pub(crate) fn valid_journal_phase(phase: &str) -> bool {
    if matches!(phase, "publication-started" | "reconciled") {
        return true;
    }
    phase.strip_prefix("operation-").is_some_and(|value| {
        value
            .parse::<u32>()
            .is_ok_and(|number| (1..=1000).contains(&number) && number.to_string() == value)
    })
}

pub(crate) fn mutation_name(identity: &JournalIdentity, sequence: u32) -> String {
    format!(
        "refs/tags/velnor-receipt/{}/{}/operation-{sequence}",
        identity.run_id, identity.run_attempt
    )
}

fn valid_mutation(
    item: &JournalMutation,
    receipt: &JournalReceipt,
    previous: Option<&JournalMutation>,
) -> bool {
    match (&item.phase, item.kind, &item.semver_evidence) {
        (JournalMutationPhase::Intended, JournalMutationKind::Upload, Some(evidence))
            if evidence.valid_for(&item.version) => {}
        (JournalMutationPhase::Intended, JournalMutationKind::Upload, _) | (_, _, Some(_)) => {
            return false;
        }
        _ => {}
    }
    let Some(package) = receipt
        .packages
        .iter()
        .find(|package| package.name == item.package && package.version == item.version)
    else {
        return false;
    };
    match (&item.phase, previous) {
        (JournalMutationPhase::Intended, None) => item.sequence == 1,
        (JournalMutationPhase::Intended, Some(prior)) => {
            matches!(prior.phase, JournalMutationPhase::Observed { .. })
        }
        (JournalMutationPhase::Observed { result }, Some(prior)) => {
            valid_mutation_result(item.kind, result, package, &receipt.identity)
                && matches!(prior.phase, JournalMutationPhase::Intended)
                && prior.package == item.package
                && prior.version == item.version
                && prior.kind == item.kind
        }
        _ => false,
    }
}

fn valid_mutation_result(
    kind: JournalMutationKind,
    result: &JournalMutationResult,
    package: &JournalPackage,
    identity: &JournalIdentity,
) -> bool {
    match (kind, result) {
        (
            JournalMutationKind::Upload,
            JournalMutationResult::Upload {
                registry_checksum,
                downloaded_checksum,
                owners,
                features,
                files,
            },
        ) => {
            schema::valid_sha256(registry_checksum)
                && registry_checksum == downloaded_checksum
                && match &package.owners {
                    JournalOwnership::Existing { owners: expected } => owners == expected,
                    JournalOwnership::NewCrate => schema::valid_owners(owners),
                }
                && schema::valid_files(files)
                && files.windows(2).all(|pair| pair[0].path < pair[1].path)
                && files
                    .iter()
                    .all(|file| matches!(file.kind, JournalFileKind::Regular { .. }))
                && features.len() <= 10_000
                && features.iter().all(|(name, values)| {
                    schema::valid_feature(name)
                        && values.len() <= 10_000
                        && values.iter().all(|value| schema::valid_feature(value))
                })
        }
        (JournalMutationKind::CreateTag, JournalMutationResult::Tag { name, target_sha }) => {
            name == &package.tag && target_sha == &identity.source_sha
        }
        (JournalMutationKind::CreateRelease, JournalMutationResult::Release { id, tag, url }) => {
            *id > 0
                && tag == &package.tag
                && [package.tag.clone(), schema::percent_encode(&package.tag)]
                    .iter()
                    .any(|tag| {
                        url == &format!(
                            "https://github.com/{}/releases/tag/{tag}",
                            identity.repository
                        )
                    })
        }
        _ => false,
    }
}

fn read_mutation<I: JournalIo>(
    io: &I,
    identity: &JournalIdentity,
    sequence: u32,
) -> Result<Option<MutationRecord>, OrchestratorError> {
    let name = mutation_name(identity, sequence);
    let Some(tag_sha) = read_ref(io, &identity.repository, &name).map_err(|_| invalid())? else {
        return Ok(None);
    };
    if !valid_sha(&tag_sha) {
        return Err(invalid());
    }
    let tag = io
        .get_tag_object(&identity.repository, &tag_sha)
        .map_err(|_| invalid())?;
    if tag.name != name.trim_start_matches("refs/tags/")
        || tag.object_type != "blob"
        || tag.message != TAG_MESSAGE
        || !valid_sha(&tag.object_sha)
    {
        return Err(invalid());
    }
    verify_raw_tag(io, &identity.repository, &tag_sha, &tag)?;
    let bytes = io
        .get_blob(&identity.repository, &tag.object_sha)
        .map_err(|_| invalid())?;
    if bytes.len() > MAX_RECORD_BYTES
        || crate::release_policy_admission::git_blob_sha1(&bytes)? != tag.object_sha
    {
        return Err(invalid());
    }
    let value =
        velnor_actions_contract::strict_json::parse_strict_json_bytes(&bytes, MAX_RECORD_BYTES)
            .map_err(|_| invalid())?;
    let record: MutationRecord = serde_json::from_value(value).map_err(|_| invalid())?;
    if record.schema != 1
        || record.identity != *identity
        || record.mutation.sequence != sequence
        || !valid_receipt(&record.receipt, identity)
        || velnor_actions_contract::canonical_json_bytes(&record).map_err(|_| invalid())? != bytes
    {
        return Err(invalid());
    }
    Ok(Some(record))
}

pub trait JournalOperationRefs {
    fn list_operation_refs(
        &self,
        identity: &JournalIdentity,
    ) -> Result<Vec<String>, OrchestratorError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PriorHistoryEvidence {
    pub start: JournalEvidence,
    pub mutations: Vec<JournalMutation>,
}

pub(crate) fn inspect_current_history<I: JournalIo + JournalOperationRefs>(
    io: &I,
    identity: &JournalIdentity,
    receipt: &JournalReceipt,
) -> Result<Vec<JournalMutation>, OrchestratorError> {
    let (mutations, completed) = inspect_recoverable_history(io, identity, receipt)?;
    if completed.is_some()
        || mutations
            .last()
            .is_some_and(|item| matches!(item.phase, JournalMutationPhase::Intended))
    {
        return Err(invalid());
    }
    Ok(mutations)
}

pub(crate) fn inspect_recoverable_history<I: JournalIo + JournalOperationRefs>(
    io: &I,
    identity: &JournalIdentity,
    receipt: &JournalReceipt,
) -> Result<(Vec<JournalMutation>, Option<JournalEvidence>), OrchestratorError> {
    let start = inspect_journal(io, identity, JournalStage::Started)?.ok_or_else(invalid)?;
    let completed = inspect_journal(io, identity, JournalStage::Completed)?;
    if start.receipt != *receipt
        || completed
            .as_ref()
            .is_some_and(|value| value.receipt != *receipt)
    {
        return Err(invalid());
    }
    let listed = operation_sequences(io, identity)?;
    let mut mutations = Vec::with_capacity(listed.len());
    for sequence in &listed {
        let record = read_mutation(io, identity, *sequence)?.ok_or_else(invalid)?;
        if record.receipt != *receipt
            || !valid_mutation(&record.mutation, receipt, mutations.last())
        {
            return Err(invalid());
        }
        mutations.push(record.mutation);
    }
    if (completed.is_some()
        && mutations
            .last()
            .is_some_and(|item| matches!(item.phase, JournalMutationPhase::Intended)))
        || operation_sequences(io, identity)? != listed
        || inspect_journal(io, identity, JournalStage::Started)?.as_ref() != Some(&start)
        || inspect_journal(io, identity, JournalStage::Completed)? != completed
    {
        return Err(invalid());
    }
    Ok((mutations, completed))
}

pub fn inspect_prior_history<I: JournalIo + JournalOperationRefs>(
    io: &I,
    previous: &JournalIdentity,
    current: &JournalReceipt,
    source: &std::path::Path,
) -> Result<PriorHistoryEvidence, OrchestratorError> {
    let start = inspect_prior_incomplete_inner(io, previous, current, source, true)?;
    let listed = operation_sequences(io, previous)?;
    let mut mutations = Vec::with_capacity(listed.len());
    for sequence in &listed {
        let record = read_mutation(io, previous, *sequence)?.ok_or_else(invalid)?;
        if record.receipt != start.receipt
            || !valid_mutation(&record.mutation, &start.receipt, mutations.last())
        {
            return Err(invalid());
        }
        mutations.push(record.mutation);
    }
    if operation_sequences(io, previous)? != listed
        || inspect_prior_incomplete_inner(io, previous, current, source, true)? != start
    {
        return Err(invalid());
    }
    verify_new_crate_prior_intentions(&start.receipt, current, &mutations)?;
    Ok(PriorHistoryEvidence { start, mutations })
}

fn operation_sequences<I: JournalOperationRefs>(
    io: &I,
    identity: &JournalIdentity,
) -> Result<Vec<u32>, OrchestratorError> {
    let prefix = format!(
        "refs/tags/velnor-receipt/{}/{}/operation-",
        identity.run_id, identity.run_attempt
    );
    let refs = io.list_operation_refs(identity)?;
    if refs.len() > 1000 {
        return Err(invalid());
    }
    let mut sequences = std::collections::BTreeSet::new();
    for reference in refs {
        let value = reference.strip_prefix(&prefix).ok_or_else(invalid)?;
        let number = value.parse::<u32>().map_err(|_| invalid())?;
        if number == 0 || number > 1000 || number.to_string() != value || !sequences.insert(number)
        {
            return Err(invalid());
        }
    }
    let values: Vec<_> = sequences.into_iter().collect();
    if values
        .iter()
        .copied()
        .ne(1..=u32::try_from(values.len()).map_err(|_| invalid())?)
    {
        return Err(invalid());
    }
    Ok(values)
}

pub fn inspect_prior_incomplete_evidence<I: JournalIo>(
    io: &I,
    previous: &JournalIdentity,
    current: &JournalReceipt,
    source: &std::path::Path,
) -> Result<JournalEvidence, OrchestratorError> {
    inspect_prior_incomplete_inner(io, previous, current, source, false)
}

fn inspect_prior_incomplete_inner<I: JournalIo>(
    io: &I,
    previous: &JournalIdentity,
    current: &JournalReceipt,
    source: &std::path::Path,
    allow_creation_transition: bool,
) -> Result<JournalEvidence, OrchestratorError> {
    validate_identity(&current.identity)?;
    if !valid_receipt(current, &current.identity)
        || current.predecessor.as_ref() != Some(previous)
        || (previous.run_id == current.identity.run_id
            && previous.run_attempt >= current.identity.run_attempt)
        || previous.repository != current.identity.repository
        || previous.source_sha != current.identity.source_sha
    {
        return Err(invalid());
    }
    crate::verify_journal_source(source, current)?;
    let prior = inspect_journal(io, previous, JournalStage::Started)?.ok_or_else(invalid)?;
    if inspect_journal(io, previous, JournalStage::Completed)?.is_some()
        || prior.receipt.base_intent_sha256 != current.base_intent_sha256
        || prior.receipt.registry != current.registry
        || prior.receipt.authentication != current.authentication
        || !prior_packages_compatible(&prior.receipt, current, allow_creation_transition)
        || prior.receipt.source_files != current.source_files
    {
        return Err(invalid());
    }
    Ok(prior)
}

fn prior_packages_compatible(
    prior: &JournalReceipt,
    current: &JournalReceipt,
    allow_creation_transition: bool,
) -> bool {
    prior.packages.len() == current.packages.len() && prior.packages.iter().zip(&current.packages).all(|(old, now)| {
        if old.name != now.name || old.version != now.version || old.manifest != now.manifest || old.tag != now.tag { return false; }
        if old.owners == now.owners {
            if matches!(current.semver_evidence.get(&now.name), Some(crate::crate_release_semver::SemverEvidence::PriorNoBaselinePending { .. })) {
                return prior.predecessor.is_some() && prior.semver_evidence.get(&old.name) == current.semver_evidence.get(&now.name);
            }
            return true;
        }
        allow_creation_transition && prior.authentication == JournalAuthentication::BootstrapToken
            && matches!(old.owners, JournalOwnership::NewCrate)
            && matches!(now.owners, JournalOwnership::Existing { .. })
            && matches!(prior.semver_evidence.get(&old.name), Some(crate::crate_release_semver::SemverEvidence::NoBaseline { .. }))
            && matches!(current.semver_evidence.get(&now.name), Some(crate::crate_release_semver::SemverEvidence::PriorNoBaselinePending { .. }))
    })
}

fn verify_new_crate_prior_intentions(
    prior: &JournalReceipt,
    current: &JournalReceipt,
    mutations: &[JournalMutation],
) -> Result<(), OrchestratorError> {
    use crate::crate_release_semver::SemverEvidence;
    for (old, now) in prior.packages.iter().zip(&current.packages) {
        if old.owners == now.owners {
            continue;
        }
        let Some(SemverEvidence::NoBaseline {
            checker_sha256: prior_checker,
        }) = prior.semver_evidence.get(&old.name)
        else {
            return Err(invalid());
        };
        let Some(SemverEvidence::PriorNoBaselinePending {
            checker_sha256,
            target_checksum,
        }) = current.semver_evidence.get(&now.name)
        else {
            return Err(invalid());
        };
        if prior_checker != checker_sha256 {
            return Err(invalid());
        }
        let candidates: Vec<_> = mutations
            .iter()
            .enumerate()
            .filter(|(_, item)| {
                item.package == old.name
                    && item.version == old.version
                    && item.kind == JournalMutationKind::Upload
                    && matches!(item.phase, JournalMutationPhase::Intended)
            })
            .collect();
        if candidates.len() != 1 {
            return Err(invalid());
        }
        let (index, intended) = candidates[0];
        if intended.semver_evidence != prior.semver_evidence.get(&old.name).cloned() {
            return Err(invalid());
        }
        if let Some(next) = mutations.get(index + 1) {
            let JournalMutationPhase::Observed {
                result:
                    JournalMutationResult::Upload {
                        registry_checksum,
                        downloaded_checksum,
                        owners,
                        ..
                    },
            } = &next.phase
            else {
                return Err(invalid());
            };
            let JournalOwnership::Existing { owners: actual } = &now.owners else {
                return Err(invalid());
            };
            if registry_checksum != target_checksum
                || downloaded_checksum != target_checksum
                || owners != actual
            {
                return Err(invalid());
            }
        }
    }
    Ok(())
}

pub(crate) fn inspect_prior_chain<I: JournalIo + JournalOperationRefs>(
    io: &I,
    current: &JournalReceipt,
    source: &std::path::Path,
) -> Result<Vec<PriorHistoryEvidence>, OrchestratorError> {
    let mut chain = Vec::new();
    let mut cursor = current.clone();
    let mut seen = std::collections::BTreeSet::new();
    seen.insert((cursor.identity.run_id, cursor.identity.run_attempt));
    while let Some(previous) = cursor.predecessor.clone() {
        if chain.len() == 32 || !seen.insert((previous.run_id, previous.run_attempt)) {
            return Err(invalid());
        }
        let evidence = inspect_prior_history(io, &previous, &cursor, source)?;
        cursor = evidence.start.receipt.clone();
        chain.push(evidence);
    }
    Ok(chain)
}

pub fn record_source_bound_start<I: JournalIo>(
    io: &I,
    identity: &JournalIdentity,
    receipt: &JournalReceipt,
    source: &std::path::Path,
) -> Result<String, OrchestratorError> {
    if &receipt.identity != identity {
        return Err(internal("crate_release_journal_source_mismatch"));
    }
    crate::verify_journal_source(source, receipt)?;
    record_journal(io, identity, JournalStage::Started, receipt, None)
}

pub(crate) fn record_reconciled_completion<J, G, R>(
    input: &crate::crate_release_controller::CrateReleaseObservation<'_, J, G, R>,
    tools: crate::package_release_signer::MeasuredReleaseTools,
) -> Result<String, OrchestratorError>
where
    J: JournalIo + JournalOperationRefs,
    G: crate::crate_release_github::GitHubAdmissionRead,
    R: crate::crate_release_registry::CrateRegistryRead,
{
    let candidate = crate::crate_release_controller::derive_reconciled_candidate(input)?;
    let completion = JournalCompletion {
        packages: crate::crate_release_controller::reconciled_completion_packages(&candidate)?,
        post_publication_file_hashes: tools.into_pins(),
    };
    record_journal(
        input.journal,
        input.identity,
        JournalStage::Completed,
        input.journal_receipt,
        Some(&completion),
    )
}

fn record_journal<I: JournalIo>(
    io: &I,
    identity: &JournalIdentity,
    stage: JournalStage,
    receipt: &JournalReceipt,
    completion: Option<&JournalCompletion>,
) -> Result<String, OrchestratorError> {
    validate_identity(identity)?;
    if !valid_receipt(receipt, identity)
        || match (stage, completion) {
            (JournalStage::Started, None) => false,
            (JournalStage::Completed, Some(value)) => !valid_completion(receipt, value, identity),
            _ => true,
        }
    {
        return Err(invalid());
    }
    let name = journal_name(identity, stage);
    io.check_tag_rules(&identity.repository)
        .map_err(|_| invalid())?;
    if read_ref(io, &identity.repository, &name)
        .map_err(|_| invalid())?
        .is_some()
    {
        let existing = read_record(io, identity, stage)?;
        if existing.receipt == *receipt && existing.completion.as_ref() == completion {
            if stage == JournalStage::Completed {
                let started = read_record(io, identity, JournalStage::Started)?;
                if started.receipt != *receipt {
                    return Err(invalid());
                }
                ensure_no_pending_mutation(io, identity, receipt)?;
            }
            io.check_tag_rules(&identity.repository)
                .map_err(|_| invalid())?;
            return Ok(name);
        }
        return Err(invalid());
    }
    if stage == JournalStage::Completed {
        let started = read_record(io, identity, JournalStage::Started)?;
        if started.receipt != *receipt {
            return Err(invalid());
        }
        ensure_no_pending_mutation(io, identity, receipt)?;
    }
    let record = JournalRecord {
        schema: 1,
        identity: identity.clone(),
        stage,
        receipt: receipt.clone(),
        completion: completion.cloned(),
    };
    let bytes = velnor_actions_contract::canonical_json_bytes(&record).map_err(|_| invalid())?;
    if bytes.len() > MAX_RECORD_BYTES {
        return Err(invalid());
    }
    let tag_sha = create_content_addressed_tag(io, identity, &name, &bytes)?;
    io.check_tag_rules(&identity.repository)
        .map_err(|_| invalid())?;
    if io
        .create_ref(&identity.repository, &name, &tag_sha)
        .is_err()
    {
        read_ref(io, &identity.repository, &name)
            .map_err(|_| internal("crate_release_journal_ref_write_ambiguous"))?;
        return Err(internal("crate_release_journal_ref_write_ambiguous"));
    }
    if read_ref(io, &identity.repository, &name)
        .map_err(|_| invalid())?
        .as_deref()
        != Some(tag_sha.as_str())
        || read_record(io, identity, stage)? != record
    {
        return Err(invalid());
    }
    io.check_tag_rules(&identity.repository)
        .map_err(|_| invalid())?;
    Ok(name)
}

fn create_content_addressed_tag<I: JournalIo>(
    io: &I,
    identity: &JournalIdentity,
    name: &str,
    bytes: &[u8],
) -> Result<String, OrchestratorError> {
    let expected_blob = crate::release_policy_admission::git_blob_sha1(bytes)?;
    let blob = io
        .create_blob(&identity.repository, bytes)
        .map_err(|_| invalid())?;
    if blob != expected_blob {
        return Err(invalid());
    }
    let tag = JournalTag {
        name: name.trim_start_matches("refs/tags/").to_owned(),
        object_type: "blob".to_owned(),
        object_sha: blob,
        message: TAG_MESSAGE.to_owned(),
    };
    let tag_sha = io
        .create_tag_object(&identity.repository, &tag)
        .map_err(|_| invalid())?;
    if !valid_sha(&tag_sha)
        || io
            .get_tag_object(&identity.repository, &tag_sha)
            .map_err(|_| invalid())?
            != tag
    {
        return Err(invalid());
    }
    verify_raw_tag(io, &identity.repository, &tag_sha, &tag)?;
    Ok(tag_sha)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JournalEvidence {
    pub receipt: JournalReceipt,
    pub completion: Option<JournalCompletion>,
}

pub fn inspect_journal<I: JournalIo>(
    io: &I,
    identity: &JournalIdentity,
    stage: JournalStage,
) -> Result<Option<JournalEvidence>, OrchestratorError> {
    validate_identity(identity)?;
    io.check_tag_rules(&identity.repository)
        .map_err(|_| invalid())?;
    if read_ref(io, &identity.repository, &journal_name(identity, stage))
        .map_err(|_| invalid())?
        .is_none()
    {
        return Ok(None);
    }
    let record = read_record(io, identity, stage)?;
    Ok(Some(JournalEvidence {
        receipt: record.receipt,
        completion: record.completion,
    }))
}

fn read_record<I: JournalIo>(
    io: &I,
    identity: &JournalIdentity,
    stage: JournalStage,
) -> Result<JournalRecord, OrchestratorError> {
    let name = journal_name(identity, stage);
    let tag_sha = read_ref(io, &identity.repository, &name)
        .map_err(|_| invalid())?
        .ok_or_else(invalid)?;
    if !valid_sha(&tag_sha) {
        return Err(invalid());
    }
    let tag = io
        .get_tag_object(&identity.repository, &tag_sha)
        .map_err(|_| invalid())?;
    if tag.name != name.trim_start_matches("refs/tags/")
        || tag.object_type != "blob"
        || tag.message != TAG_MESSAGE
        || !valid_sha(&tag.object_sha)
    {
        return Err(invalid());
    }
    verify_raw_tag(io, &identity.repository, &tag_sha, &tag)?;
    let bytes = io
        .get_blob(&identity.repository, &tag.object_sha)
        .map_err(|_| invalid())?;
    if bytes.len() > MAX_RECORD_BYTES
        || crate::release_policy_admission::git_blob_sha1(&bytes)? != tag.object_sha
    {
        return Err(invalid());
    }
    let value =
        velnor_actions_contract::strict_json::parse_strict_json_bytes(&bytes, MAX_RECORD_BYTES)
            .map_err(|_| invalid())?;
    let record: JournalRecord = serde_json::from_value(value).map_err(|_| invalid())?;
    if record.schema != 1
        || record.identity != *identity
        || record.stage != stage
        || !valid_receipt(&record.receipt, identity)
        || match (record.stage, &record.completion) {
            (JournalStage::Started, None) => false,
            (JournalStage::Completed, Some(value)) => {
                !valid_completion(&record.receipt, value, identity)
            }
            _ => true,
        }
        || velnor_actions_contract::canonical_json_bytes(&record).map_err(|_| invalid())? != bytes
    {
        return Err(invalid());
    }
    Ok(record)
}

fn journal_name(identity: &JournalIdentity, stage: JournalStage) -> String {
    let phase = match stage {
        JournalStage::Started => "publication-started",
        JournalStage::Completed => "reconciled",
    };
    format!(
        "refs/tags/velnor-receipt/{}/{}/{phase}",
        identity.run_id, identity.run_attempt
    )
}

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
    io.check_repository_access(repository)
        .map_err(|_| invalid())?;
    let observed = io.get_ref(repository, reference).map_err(|_| invalid())?;
    if observed.is_none() {
        io.check_repository_access(repository)
            .map_err(|_| invalid())?;
    }
    Ok(observed)
}

#[cfg(test)]
#[path = "../tests/crate_release_journal_public.rs"]
mod tests;

#[cfg(test)]
pub(crate) fn fixture_record_journal_mutation<I: JournalIo>(
    io: &I,
    identity: &JournalIdentity,
    receipt: &JournalReceipt,
    mutation: &JournalMutation,
) -> Result<String, OrchestratorError> {
    record_journal_mutation(io, identity, receipt, mutation)
}
