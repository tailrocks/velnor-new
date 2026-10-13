//! External-data freshness for skippable checks (par §5).
//!
//! A check backed by changing external data (such as an advisory
//! database) may be skipped only when its obligation carries the data
//! identity and the baseline proof is within the maximum age;
//! otherwise the check runs again.

use serde::{Deserialize, Serialize};
use velnor_actions_contract::{ContractError, validate_digest};

use crate::extension_schemas::task_kind_segment;

/// Task-kind segment marking an external-data-backed check.
pub const EXTERNAL_DATA_CHECK_KIND: &str = "advisory";

/// Default maximum baseline age for external-data checks (24 hours).
pub const DEFAULT_EXTERNAL_DATA_MAX_AGE_SECS: u64 = 86_400;

/// External-data identity and freshness carried by a baseline proof.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExternalDataFreshness {
    /// Data source name (such as an advisory database).
    pub source: String,
    /// Digest over the exact external data observed.
    pub identity: String,
    /// Proof age in seconds when the baseline was recorded.
    pub age_secs: u64,
}

impl ExternalDataFreshness {
    /// Validate source shape and identity digest.
    /// # Errors
    pub fn validate(&self) -> Result<(), ContractError> {
        if self.source.trim().is_empty()
            || self.source.contains('/')
            || self.source.contains(char::is_whitespace)
        {
            return Err(ContractError::identity(
                "external_data.source",
                "bad_source",
            ));
        }
        validate_digest(&self.identity)?;
        Ok(())
    }
}

/// External-data kind of `task_id`, when it names a backed check.
#[must_use]
pub fn external_data_kind(task_id: &str) -> Option<&str> {
    task_kind_segment(task_id).filter(|kind| *kind == EXTERNAL_DATA_CHECK_KIND)
}

/// True only when the obligation declares the data identity and the
/// baseline proof is present, valid, and within `max_age_secs`.
#[must_use]
pub fn may_skip_external_data(
    declared_in_obligation: bool,
    baseline: Option<&ExternalDataFreshness>,
    max_age_secs: u64,
) -> bool {
    if !declared_in_obligation {
        return false;
    }
    baseline.is_some_and(|proof| proof.age_secs <= max_age_secs && proof.validate().is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use velnor_actions_contract::digest_b3;

    /// Freshness proof with `age_secs` over a fixed identity.
    fn proof(age_secs: u64) -> ExternalDataFreshness {
        ExternalDataFreshness {
            source: "advisory-db".to_owned(),
            identity: digest_b3(b"snapshot"),
            age_secs,
        }
    }

    #[test]
    fn skip_needs_declared_identity_and_fresh_baseline() {
        let fresh = proof(60);
        assert!(may_skip_external_data(true, Some(&fresh), 86_400));
        assert!(!may_skip_external_data(false, Some(&fresh), 86_400));
        assert!(!may_skip_external_data(true, None, 86_400));
        assert!(!may_skip_external_data(true, Some(&proof(86_401)), 86_400));
        let mut bad = proof(60);
        bad.identity = "not-a-digest".to_owned();
        assert!(!may_skip_external_data(true, Some(&bad), 86_400));
    }

    #[test]
    fn advisory_kind_classifies_stack_and_internal_ids() {
        assert_eq!(
            external_data_kind("stack/rust/root/advisory/default"),
            Some("advisory")
        );
        assert_eq!(
            external_data_kind("internal/advisory/default"),
            Some("advisory")
        );
        assert_eq!(external_data_kind("stack/rust/root/clippy/default"), None);
        assert_eq!(external_data_kind("bogus"), None);
    }

    #[test]
    fn freshness_validation_rejects_bad_source_and_identity() {
        assert!(proof(0).validate().is_ok());
        let mut bad = proof(0);
        bad.source = String::new();
        assert!(bad.validate().is_err());
        bad.source = "has space".to_owned();
        assert!(bad.validate().is_err());
        let mut bad = proof(0);
        bad.identity = "b3-short".to_owned();
        assert!(bad.validate().is_err());
    }
}
