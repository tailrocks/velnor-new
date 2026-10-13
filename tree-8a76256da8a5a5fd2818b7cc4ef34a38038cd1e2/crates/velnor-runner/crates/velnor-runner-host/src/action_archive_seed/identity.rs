use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{ActionArchiveSeedError, FORMAT_VERSION, RUNNER_ARCHIVE_LAYOUT, TRUST_SCOPE};

const MAX_ARCHIVE_BYTES: u64 = 512 * 1024 * 1024;

/// Exact source identity and bytes for one resolved Linux action archive.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ActionArchiveIdentity {
    /// Numeric GitHub repository identity. This prevents name reuse from aliasing a source.
    pub(crate) repository_id: u64,
    /// Exact `owner/repository` value used by the pinned runner cache layout.
    pub(crate) name_with_owner: String,
    /// Full resolved commit SHA. Tags and branch names are rejected.
    pub(crate) commit_sha: String,
    /// SHA-256 of the compressed archive bytes.
    pub(crate) sha256: [u8; 32],
    /// Compressed archive length in bytes.
    pub(crate) size: u64,
}

#[derive(Serialize)]
struct ObjectGeneration<'a> {
    format_version: u32,
    trust_scope: &'static str,
    runner_archive_layout: &'static str,
    identity: &'a ActionArchiveIdentity,
}

#[derive(Serialize)]
struct LeaseGeneration<'a> {
    format_version: u32,
    trust_scope: &'static str,
    runner_archive_layout: &'static str,
    consumer_repository_id: u64,
    archives: &'a [ActionArchiveIdentity],
}

pub(super) fn normalized_allowlist(
    allowlist: &[ActionArchiveIdentity],
) -> Result<Vec<ActionArchiveIdentity>, ActionArchiveSeedError> {
    let mut archives = allowlist.to_vec();
    for identity in &archives {
        validate_identity(identity)?;
    }
    archives.sort_by(|left, right| {
        (&left.name_with_owner, &left.commit_sha, left.repository_id).cmp(&(
            &right.name_with_owner,
            &right.commit_sha,
            right.repository_id,
        ))
    });
    let mut seen = BTreeSet::new();
    for identity in &archives {
        let (owner_repo, sha) = runner_relative_path(identity)?;
        if !seen.insert(format!("{owner_repo}/{sha}").to_ascii_lowercase()) {
            return Err(ActionArchiveSeedError::InvalidIdentity);
        }
    }
    Ok(archives)
}

pub(super) fn validate_identity(
    identity: &ActionArchiveIdentity,
) -> Result<(), ActionArchiveSeedError> {
    if identity.repository_id == 0
        || identity.size == 0
        || identity.size > MAX_ARCHIVE_BYTES
        || !valid_repo_name(&identity.name_with_owner)
        || identity.commit_sha.len() != 40
        || !identity
            .commit_sha
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(ActionArchiveSeedError::InvalidIdentity);
    }
    Ok(())
}

pub(super) fn validate_component(value: &str) -> Result<(), ActionArchiveSeedError> {
    if value.is_empty()
        || value.len() > 160
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"-_".contains(&byte))
    {
        return Err(ActionArchiveSeedError::InvalidLease);
    }
    Ok(())
}

pub(super) fn validate_generation_id(value: &str) -> Result<(), ActionArchiveSeedError> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(ActionArchiveSeedError::InvalidLease);
    }
    Ok(())
}

pub(super) fn runner_relative_path(
    identity: &ActionArchiveIdentity,
) -> Result<(String, String), ActionArchiveSeedError> {
    validate_identity(identity)?;
    Ok((
        identity.name_with_owner.replace('/', "_"),
        format!("{}.tar.gz", identity.commit_sha),
    ))
}

pub(super) fn object_generation(
    identity: &ActionArchiveIdentity,
) -> Result<String, ActionArchiveSeedError> {
    validate_identity(identity)?;
    let bytes = serde_json::to_vec(&ObjectGeneration {
        format_version: FORMAT_VERSION,
        trust_scope: TRUST_SCOPE,
        runner_archive_layout: RUNNER_ARCHIVE_LAYOUT,
        identity,
    })
    .map_err(|_| ActionArchiveSeedError::Manifest)?;
    Ok(hex_digest(&Sha256::digest(bytes)))
}

pub(super) fn lease_generation(
    consumer_repository_id: u64,
    archives: &[ActionArchiveIdentity],
) -> Result<String, ActionArchiveSeedError> {
    let bytes = serde_json::to_vec(&LeaseGeneration {
        format_version: FORMAT_VERSION,
        trust_scope: TRUST_SCOPE,
        runner_archive_layout: RUNNER_ARCHIVE_LAYOUT,
        consumer_repository_id,
        archives,
    })
    .map_err(|_| ActionArchiveSeedError::Manifest)?;
    Ok(hex_digest(&Sha256::digest(bytes)))
}

pub(super) fn valid_repo_name(name: &str) -> bool {
    let mut parts = name.split('/');
    let Some(owner) = parts.next() else {
        return false;
    };
    let Some(repo) = parts.next() else {
        return false;
    };
    parts.next().is_none() && valid_name_part(owner) && valid_name_part(repo)
}

fn valid_name_part(part: &str) -> bool {
    !part.is_empty()
        && part != "."
        && part != ".."
        && part
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_.-".contains(&byte))
}

fn hex_digest(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut result = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        result.push(char::from(HEX[usize::from(byte >> 4)]));
        result.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    result
}
