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
    /// Observed full ref name, such as `refs/heads/main`, when the task reads it.
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
mod tests {
    use std::collections::BTreeMap;

    use super::VcsInputs;

    #[test]
    fn commit_width_rejects_empty_and_uppercase() {
        let vcs = |commit: Option<&str>| VcsInputs {
            commit: commit.map(str::to_owned),
            reference: None,
            submodules: BTreeMap::new(),
        };
        assert!(vcs(Some(&"a".repeat(40))).validate().is_ok());
        assert!(vcs(Some("")).validate().is_err());
        assert!(vcs(Some(&"A".repeat(40))).validate().is_err());
        assert!(vcs(Some("abc")).validate().is_err());
    }

    #[test]
    fn reference_accepts_git_valid_full_refs() {
        for reference in [
            "refs/heads/main",
            "refs/heads/release/1.2",
            "refs/heads/feature/x_y-z",
            "refs/heads/main.LOCK",
            "refs/heads/a/b.Lock",
            "refs/remotes/origin/a/-nested",
            "refs/heads/a@b",
            "refs/heads/@",
            "refs/heads/a]b",
            "refs/heads/a./b",
            "refs/heads/máin",
            "refs/heads/-main",
            "refs/heads/HEAD",
            "refs/tags/v1.0+meta",
        ] {
            let vcs = VcsInputs {
                commit: None,
                reference: Some(reference.to_owned()),
                submodules: BTreeMap::new(),
            };
            assert!(vcs.validate().is_ok(), "{reference:?}");
        }
    }

    #[test]
    fn reference_rejects_git_invalid_and_special_names() {
        for reference in [
            "",
            "main",
            "HEAD",
            "@",
            "/refs/heads/main",
            "refs/heads/main/",
            "refs//heads/main",
            "refs/heads/.hidden",
            "refs/heads/main.",
            "refs/heads/main.lock",
            "refs/heads/a/b.lock",
            "refs/heads/a.lock/child",
            "refs/heads/a..b",
            "refs/heads/a/../b",
            "refs/heads/a/./b",
            "refs/heads/a@{b",
            "@{-1}",
            "refs/heads/a b",
            "refs/heads/a\nb",
            "refs/heads/a\tb",
            "refs/heads/a\0b",
            "refs/heads/a\u{7f}b",
            "refs/heads/a~b",
            "refs/heads/a^b",
            "refs/heads/a:b",
            "refs/heads/a?b",
            "refs/heads/a*b",
            "refs/heads/a[b",
            "refs/heads/a\\b",
        ] {
            let vcs = VcsInputs {
                commit: None,
                reference: Some(reference.to_owned()),
                submodules: BTreeMap::new(),
            };
            assert!(vcs.validate().is_err(), "{reference:?}");
        }
    }
}
