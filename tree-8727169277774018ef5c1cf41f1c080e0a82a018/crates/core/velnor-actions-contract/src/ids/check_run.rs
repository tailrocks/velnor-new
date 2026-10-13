//! Canonical GitHub Check Run identity parsing.

use std::num::NonZeroI64;

use crate::ContractError;

/// A positive Check Run ID parsed from a canonical public GitHub repository URL.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CheckRunId(NonZeroI64);

impl CheckRunId {
    /// Parse a canonical public GitHub Check Run URL for the expected repository.
    ///
    /// Owner and repository comparisons are ASCII-path-segment checked and
    /// case-insensitive, matching GitHub's canonical repository URLs.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid scope segments, a different origin or
    /// repository, a noncanonical path or numeric ID, or an out-of-range ID.
    pub fn parse_api_url(url: &str, owner: &str, repository: &str) -> Result<Self, ContractError> {
        let bad = || ContractError::identity("check_run_id", "malformed_check_run_url");
        if !valid_repository_segment(owner) || !valid_repository_segment(repository) {
            return Err(bad());
        }
        let scoped_path = url
            .strip_prefix("https://api.github.com/repos/")
            .ok_or_else(bad)?;
        let (url_owner, remainder) = scoped_path.split_once('/').ok_or_else(bad)?;
        let (url_repository, check_run_path) = remainder.split_once('/').ok_or_else(bad)?;
        if !valid_repository_segment(url_owner)
            || !valid_repository_segment(url_repository)
            || !url_owner.eq_ignore_ascii_case(owner)
            || !url_repository.eq_ignore_ascii_case(repository)
        {
            return Err(bad());
        }
        let id = check_run_path.strip_prefix("check-runs/").ok_or_else(bad)?;
        if id.is_empty() || id.starts_with('0') || !id.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(bad());
        }
        let id = id.parse::<i64>().map_err(|_| bad())?;
        NonZeroI64::new(id).map(Self).ok_or_else(bad)
    }

    /// Return the validated positive numeric ID.
    #[must_use]
    pub const fn get(self) -> NonZeroI64 {
        self.0
    }
}

fn valid_repository_segment(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 100
        && value != "."
        && value != ".."
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

#[cfg(test)]
mod tests;
