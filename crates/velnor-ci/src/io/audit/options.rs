// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::CiError;

/// Entries one audit's freshness scans may walk when `VELNOR_AUDIT_MAX_ENTRIES` is unset.
/// A workspace's sources and its Cargo roots stay far below it; the bound stops a runaway tree.
pub(super) const DEFAULT_MAX_ENTRIES: usize = 1_000_000;
/// The largest value `VELNOR_AUDIT_MAX_ENTRIES` may set.
pub(super) const MAX_ENTRIES_CEILING: usize = 10_000_000;
pub(super) const MAX_ENTRIES_VARIABLE: &str = "VELNOR_AUDIT_MAX_ENTRIES";

/// The entry limit from `VELNOR_AUDIT_MAX_ENTRIES`, or the default when it is unset or empty.
pub(crate) fn max_entries_from_env() -> Result<usize, CiError> {
    max_entries_from(std::env::var(MAX_ENTRIES_VARIABLE).ok().as_deref())
}

fn max_entries_from(value: Option<&str>) -> Result<usize, CiError> {
    let Some(value) = value.filter(|value| !value.is_empty()) else {
        return Ok(DEFAULT_MAX_ENTRIES);
    };
    value
        .parse::<usize>()
        .ok()
        .filter(|limit| (1..=MAX_ENTRIES_CEILING).contains(limit))
        .ok_or_else(|| {
            CiError::new(format!(
                "{MAX_ENTRIES_VARIABLE} must be a number from 1 to {MAX_ENTRIES_CEILING}, not {value:?}"
            ))
        })
}

#[derive(Debug, Clone)]
pub struct AuditOptions {
    pub write: bool,
    pub root: Option<String>,
    pub max_directory_entries: usize,
    pub inherit_target_dir: bool,
}

impl Default for AuditOptions {
    fn default() -> Self {
        Self {
            write: false,
            root: None,
            max_directory_entries: DEFAULT_MAX_ENTRIES,
            inherit_target_dir: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_admits_a_workspace_far_above_the_old_bound() {
        assert_eq!(AuditOptions::default().max_directory_entries, 1_000_000);
        assert_eq!(max_entries_from(None).ok(), Some(1_000_000));
        assert_eq!(max_entries_from(Some("")).ok(), Some(1_000_000));
    }

    #[test]
    fn a_value_in_range_is_the_limit_and_one_outside_is_refused() {
        assert_eq!(max_entries_from(Some("1")).ok(), Some(1));
        assert_eq!(max_entries_from(Some("10000000")).ok(), Some(10_000_000));
        for value in ["0", "10000001", "many", "-1"] {
            let error = max_entries_from(Some(value)).err();
            assert!(
                error.is_some_and(|error| error.to_string().contains(MAX_ENTRIES_VARIABLE)),
                "{value}"
            );
        }
    }
}
