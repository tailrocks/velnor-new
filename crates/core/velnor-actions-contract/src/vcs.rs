//! VCS revision inputs for tasks that observe Git state (par §4.2).
//!
//! Tasks that read Git commit/ref, submodule state, or generated version
//! metadata declare those values here; they participate in the digest so a
//! revision change disables reuse.

use std::collections::BTreeMap;

use crate::canonical::{normalize_posix_path, validate_digest};
use crate::errors::ContractError;

/// VCS revision inputs block of [`TaskIdentity`](crate::canonical::TaskIdentity).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct VcsInputs {
    /// Observed commit SHA (40 lowercase hex), when the task reads it.
    pub commit: Option<String>,
    /// Observed ref name, when the task reads it.
    pub reference: Option<String>,
    /// Submodule path to content digest.
    pub submodules: BTreeMap<String, String>,
}

impl VcsInputs {
    /// Validate commit shape, ref name, and submodule digests.
    /// # Errors
    pub fn validate(&self) -> Result<(), ContractError> {
        if let Some(commit) = &self.commit {
            let sha = crate::ids::is_lower_hex_len(commit, 40);
            if !sha {
                return Err(ContractError::identity("vcs.commit", "malformed_commit"));
            }
        }
        if let Some(reference) = &self.reference
            && !is_valid_git_ref_name(reference)
        {
            return Err(ContractError::identity("vcs.reference", "malformed_ref"));
        }
        for (path, digest) in &self.submodules {
            normalize_posix_path(path)?;
            validate_digest(digest)?;
        }
        Ok(())
    }
}

/// Validate a literal full ref using Git's `check-ref-format` rules.
///
/// A slash is required. Checkout shorthand like `@{-1}` is rejected because
/// this field stores a literal observed ref name.
fn is_valid_git_ref_name(value: &str) -> bool {
    let bytes = value.as_bytes();
    if value.is_empty() || value == "@" || !value.contains('/') {
        return false;
    }
    if value.starts_with('/')
        || value.ends_with('/')
        || value.contains("//")
        || value.ends_with('.')
    {
        return false;
    }
    if bytes.windows(2).any(|pair| pair == b".." || pair == b"@{") {
        return false;
    }
    if bytes.iter().any(|byte| {
        *byte <= b' '
            || *byte == 0x7f
            || matches!(*byte, b'~' | b'^' | b':' | b'?' | b'*' | b'[' | b'\\')
    }) {
        return false;
    }
    value.split('/').all(|component| {
        !component.starts_with('.')
            && component
                .rsplit_once('.')
                .is_none_or(|(_, suffix)| suffix != "lock")
    })
}

#[cfg(test)]
mod tests;
