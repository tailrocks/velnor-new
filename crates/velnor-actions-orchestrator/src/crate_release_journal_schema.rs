// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::JournalIdentity;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[path = "crate_release_journal_source_files.rs"]
mod source_files;
pub(crate) use source_files::valid_source_files;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum JournalAuthentication {
    TrustedPublishing,
    BootstrapToken,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum JournalRegistry {
    CratesIo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum JournalOwnerKind {
    User,
    Team,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JournalOwner {
    pub id: u64,
    pub login: String,
    pub kind: JournalOwnerKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum JournalOwnership {
    NewCrate,
    Existing { owners: Vec<JournalOwner> },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JournalPackage {
    pub name: String,
    pub version: String,
    pub manifest: String,
    pub tag: String,
    pub owners: JournalOwnership,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JournalReceipt {
    pub identity: JournalIdentity,
    pub predecessor: Option<JournalIdentity>,
    pub intent_sha256: String,
    pub base_intent_sha256: String,
    pub registry: JournalRegistry,
    pub generator_version: String,
    pub release_plz_version: String,
    pub rust_version: String,
    pub expected_tools: JournalToolPins,
    pub authentication: JournalAuthentication,
    pub required_check_id: u64,
    pub trusted_reviewer_id: u64,
    pub workflow_run_url: String,
    pub source_files: Vec<JournalFile>,
    pub packages: Vec<JournalPackage>,
    pub semver_evidence:
        std::collections::BTreeMap<String, crate::release::crate_release_semver::SemverEvidence>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JournalToolPins {
    pub generator_sha256: String,
    pub release_plz_sha256: String,
    pub rust_manifest_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JournalFile {
    pub path: String,
    pub sha256: String,
    pub kind: JournalFileKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case", deny_unknown_fields)]
pub enum JournalFileKind {
    Regular { executable: bool },
    Symlink { target: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum JournalOperation {
    ObservedPresent,
    Published,
    AlreadyVerified,
    TagCreated,
    ReleaseCreated,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JournalPublishedPackage {
    pub name: String,
    pub version: String,
    pub owners: Vec<JournalOwner>,
    pub registry_checksum: String,
    pub downloaded_checksum: String,
    pub tag_target: String,
    pub release_id: u64,
    pub release_url: String,
    pub files: Vec<JournalFile>,
    pub features: BTreeMap<String, Vec<String>>,
    pub operations: Vec<JournalOperation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JournalCompletion {
    pub packages: Vec<JournalPublishedPackage>,
    pub post_publication_file_hashes: JournalToolPins,
}

pub(super) fn valid_receipt(receipt: &JournalReceipt, identity: &JournalIdentity) -> bool {
    &receipt.identity == identity
        && receipt.predecessor.as_ref().is_none_or(|previous| {
            previous.repository == identity.repository
                && previous.source_sha == identity.source_sha
                && (previous.run_id != identity.run_id
                    || previous.run_attempt < identity.run_attempt)
                && super::validate_identity(previous).is_ok()
        })
        && valid_sha256(&receipt.intent_sha256)
        && valid_sha256(&receipt.base_intent_sha256)
        && valid_version(&receipt.generator_version)
        && valid_sha256(&receipt.expected_tools.generator_sha256)
        && valid_version(&receipt.release_plz_version)
        && valid_sha256(&receipt.expected_tools.release_plz_sha256)
        && valid_version(&receipt.rust_version)
        && valid_sha256(&receipt.expected_tools.rust_manifest_sha256)
        && receipt.required_check_id > 0
        && receipt.trusted_reviewer_id > 0
        && receipt.workflow_run_url
            == format!(
                "https://github.com/{}/actions/runs/{}",
                identity.repository, identity.run_id
            )
        && valid_source_files(&receipt.source_files)
        && !receipt.packages.is_empty()
        && receipt.packages.len() <= 256
        && unique(receipt.packages.iter().map(|package| package.name.as_str()))
        && receipt.packages.iter().all(|package| {
            valid_package(package)
                && (!matches!(package.owners, JournalOwnership::NewCrate)
                    || receipt.authentication == JournalAuthentication::BootstrapToken)
        })
        && valid_semver_evidence(receipt)
}

fn valid_semver_evidence(receipt: &JournalReceipt) -> bool {
    if receipt.semver_evidence.len() != receipt.packages.len() {
        return false;
    }
    receipt.packages.iter().enumerate().all(|(index, package)| {
        let Some(evidence) = receipt.semver_evidence.get(&package.name) else {
            return false;
        };
        match evidence {
            crate::release::crate_release_semver::SemverEvidence::Deferred { predecessors } => {
                !predecessors.is_empty()
                    && predecessors.len() <= index
                    && predecessors.windows(2).all(|pair| pair[0] < pair[1])
                    && predecessors.iter().all(|name| {
                        receipt.packages[..index]
                            .iter()
                            .any(|prior| &prior.name == name)
                    })
            }
            crate::release::crate_release_semver::SemverEvidence::PriorNoBaselinePending {
                checker_sha256,
                target_checksum,
            } => {
                receipt.predecessor.is_some()
                    && receipt.authentication == JournalAuthentication::BootstrapToken
                    && matches!(package.owners, JournalOwnership::Existing { .. })
                    && valid_sha256(target_checksum)
                    && velnor_actions_mise::catalog::qualified_cargo_semver_checks("ubuntu-26.04")
                        .is_some_and(|tool| checker_sha256 == tool.executable_sha256)
            }
            crate::release::crate_release_semver::SemverEvidence::MetadataOnly {
                package: name,
                version,
                source_sha,
                intent_sha256,
                target_checksum,
            } => {
                name == &package.name
                    && version == &package.version
                    && source_sha == &receipt.identity.source_sha
                    && intent_sha256 == &receipt.intent_sha256
                    && matches!(package.owners, JournalOwnership::Existing { .. })
                    && valid_sha256(target_checksum)
            }
            _ => evidence.valid_for(&package.version),
        }
    })
}

fn valid_package(package: &JournalPackage) -> bool {
    valid_name(&package.name)
        && valid_version(&package.version)
        && valid_path(&package.manifest)
        && package.tag == format!("{}-v{}", package.name, package.version)
        && valid_tag(&package.tag)
        && match &package.owners {
            JournalOwnership::NewCrate => true,
            JournalOwnership::Existing { owners } => valid_owners(owners),
        }
}

pub(super) fn valid_owners(owners: &[JournalOwner]) -> bool {
    !owners.is_empty()
        && owners.len() <= 256
        && owners
            .iter()
            .all(|owner| owner.id > 0 && valid_owner(&owner.login))
        && owners.windows(2).all(|pair| {
            (
                pair[0].id,
                pair[0].login.as_str(),
                owner_kind_key(pair[0].kind),
            ) < (
                pair[1].id,
                pair[1].login.as_str(),
                owner_kind_key(pair[1].kind),
            )
        })
        && owners
            .iter()
            .map(|owner| (owner.id, owner_kind_key(owner.kind)))
            .collect::<BTreeSet<_>>()
            .len()
            == owners.len()
}

fn owner_kind_key(kind: JournalOwnerKind) -> &'static str {
    match kind {
        JournalOwnerKind::User => "user",
        JournalOwnerKind::Team => "team",
    }
}

pub(super) fn valid_completion(
    receipt: &JournalReceipt,
    completion: &JournalCompletion,
    identity: &JournalIdentity,
) -> bool {
    completion.post_publication_file_hashes == receipt.expected_tools
        && completion.packages.len() == receipt.packages.len()
        && receipt.packages.iter().all(|planned| {
            let matches: Vec<_> = completion
                .packages
                .iter()
                .filter(|item| item.name == planned.name && item.version == planned.version)
                .collect();
            matches.len() == 1 && valid_result(matches[0], planned, identity)
        })
}

fn valid_result(
    package: &JournalPublishedPackage,
    planned: &JournalPackage,
    identity: &JournalIdentity,
) -> bool {
    valid_sha256(&package.registry_checksum)
        && valid_owners(&package.owners)
        && match &planned.owners {
            JournalOwnership::NewCrate => true,
            JournalOwnership::Existing { owners } => owners == &package.owners,
        }
        && package.registry_checksum == package.downloaded_checksum
        && package.tag_target == identity.source_sha
        && package.release_id > 0
        && [planned.tag.clone(), percent_encode(&planned.tag)]
            .iter()
            .any(|tag| {
                package.release_url
                    == format!(
                        "https://github.com/{}/releases/tag/{tag}",
                        identity.repository
                    )
            })
        && valid_files(&package.files)
        && package
            .files
            .iter()
            .all(|file| matches!(file.kind, JournalFileKind::Regular { .. }))
        && package.features.len() <= 10_000
        && package.features.iter().all(|(name, values)| {
            valid_feature(name)
                && values.len() <= 10_000
                && values.iter().all(|value| valid_feature(value))
        })
        && !package.operations.is_empty()
        && package.operations.len() <= 4
}

pub(super) fn valid_files(files: &[JournalFile]) -> bool {
    !files.is_empty()
        && files.len() <= 100_000
        && unique(files.iter().map(|file| file.path.as_str()))
        && files
            .iter()
            .all(|file| valid_path(&file.path) && valid_sha256(&file.sha256))
}

pub(super) fn percent_encode(value: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut encoded = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(char::from(byte));
        } else {
            encoded.push('%');
            encoded.push(char::from(HEX[usize::from(byte >> 4)]));
            encoded.push(char::from(HEX[usize::from(byte & 15)]));
        }
    }
    encoded
}

pub(super) fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
fn valid_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_".contains(&byte))
}
fn valid_version(value: &str) -> bool {
    value.len() <= 150
        && semver::Version::parse(value).is_ok_and(|parsed| parsed.to_string() == value)
}

fn valid_owner(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_:".contains(&byte))
}
fn valid_tag(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_.+/".contains(&byte))
        && !value.contains("..")
        && !value.starts_with('/')
        && !value.ends_with('/')
}
fn valid_path(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 1024
        && !value.starts_with('/')
        && !value.contains('\\')
        && value.split('/').all(|part| {
            !part.is_empty() && part != "." && part != ".." && !part.chars().any(char::is_control)
        })
}
pub(super) fn valid_feature(value: &str) -> bool {
    !value.is_empty() && value.len() <= 512 && !value.chars().any(char::is_control)
}
fn unique<'a, T: Ord + 'a>(values: impl Iterator<Item = T>) -> bool {
    let mut seen = BTreeSet::new();
    values.into_iter().all(|value| seen.insert(value))
}
