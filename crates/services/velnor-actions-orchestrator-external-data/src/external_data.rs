//! External-data freshness for skippable checks (par §5).
//!
//! A check backed by changing external data (such as an advisory
//! database) may be skipped only when its obligation carries the data
//! identity and the baseline proof is within the maximum age;
//! otherwise the check runs again.

use serde::{Deserialize, Serialize};
use velnor_actions_contract::{ContractError, validate_digest};

use velnor_actions_orchestrator_core::extension_schemas::task_kind_segment;

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
mod tests;
