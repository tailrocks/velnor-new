use std::collections::BTreeMap;

use crate::canonical::{normalize_posix_path, validate_digest};
use crate::errors::ContractError;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct VcsInputs {
    pub commit: Option<String>,

    pub reference: Option<String>,

    pub submodules: BTreeMap<String, String>,
}

impl VcsInputs {
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

#[must_use]
pub fn is_safe_github_repository(value: &str) -> bool {
    let Some((owner, repository)) = value.split_once('/') else {
        return false;
    };
    [owner, repository].into_iter().all(|part| {
        !part.is_empty()
            && part.len() <= 100
            && !matches!(part, "." | "..")
            && part
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    })
}
