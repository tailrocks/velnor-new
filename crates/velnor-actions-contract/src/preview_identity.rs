//! Deterministic preview identity derived from authenticated source inputs.

use crate::{ContractError, config::is_exact_semver};

/// Immutable version and tag for one full-history default-branch commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewIdentity {
    /// Application version supplied by Velnor to the producer.
    pub version: String,
    /// Native immutable release tag; never a rolling locator.
    pub tag: String,
    /// Complete source identity, retained despite the abbreviated version suffix.
    pub source_sha: String,
    /// Full-history commit count, not a run number or timestamp.
    pub commit_count: u64,
}

impl PreviewIdentity {
    /// Compute the sole preview identity for already verified source inputs.
    ///
    /// This pure calculation does not establish CI success, ancestry, full
    /// history, or workflow authority. Eligibility must authenticate those
    /// inputs before any release mutation.
    ///
    /// # Errors
    ///
    /// Rejects prerelease/build base versions, noncanonical SHA-1 identities,
    /// and a zero commit count.
    pub fn derive(base: &str, source_sha: &str, commit_count: u64) -> Result<Self, ContractError> {
        if !is_exact_semver(base) {
            return Err(ContractError::config(
                "release",
                "base_version",
                "preview_requires_numeric_base_version",
            ));
        }
        if source_sha.len() != 40
            || !source_sha
                .bytes()
                .all(|b| b.is_ascii_digit() || matches!(b, b'a'..=b'f'))
        {
            return Err(ContractError::config(
                "release",
                "source_sha",
                "requires_full_lowercase_commit_sha",
            ));
        }
        if commit_count == 0 {
            return Err(ContractError::config(
                "release",
                "commit_count",
                "requires_full_history_count",
            ));
        }
        let version = format!("{base}-preview.{commit_count}+{}", &source_sha[..7]);
        Ok(Self {
            tag: format!("v{version}"),
            version,
            source_sha: source_sha.to_owned(),
            commit_count,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::PreviewIdentity;

    const SHA: &str = "a12e73afd75b944a872e8295d61a15f93f5789a2";

    #[test]
    fn retries_have_identical_version_and_owned_tag() {
        let identity = PreviewIdentity::derive("0.6.4", SHA, 1306).expect("qualified inputs");
        assert_eq!(identity.version, "0.6.4-preview.1306+a12e73a");
        assert_eq!(identity.tag, "v0.6.4-preview.1306+a12e73a");
        assert_eq!(identity.source_sha, SHA);
        assert_eq!(
            identity,
            PreviewIdentity::derive("0.6.4", SHA, 1306).expect("retry")
        );
    }

    #[test]
    fn identity_rejects_ambiguous_base_and_source_inputs() {
        for base in ["0.6.4-preview.1", "0.6.4+local", "v0.6.4", "00.6.4", "0.6"] {
            assert!(PreviewIdentity::derive(base, SHA, 1306).is_err(), "{base}");
        }
        for sha in [
            "a12e73a",
            "A12E73AFD75B944A872E8295D61A15F93F5789A2",
            "../main",
            "",
        ] {
            assert!(
                PreviewIdentity::derive("0.6.4", sha, 1306).is_err(),
                "{sha}"
            );
        }
        assert!(PreviewIdentity::derive("0.6.4", SHA, 0).is_err());
    }

    #[test]
    fn abbreviated_sha_collision_does_not_collapse_source_identity() {
        let first = PreviewIdentity::derive("0.6.4", SHA, 1306).expect("first");
        let second = PreviewIdentity::derive("0.6.4", &format!("a12e73a{}", "0".repeat(33)), 1307)
            .expect("second");
        assert_ne!(first.tag, second.tag);
        assert_ne!(first.source_sha, second.source_sha);
    }
}
