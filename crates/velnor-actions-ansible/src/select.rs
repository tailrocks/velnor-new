//! Affected-root claims (hard decision 3 for the Ansible stack).
//!
//! A root job is selected when a changed path lies in the root directory, is
//! the Python requirements file or the inventory (either may live outside the
//! root), or is the repository `mise.toml` family (handled by the declared
//! version comparison, not here).

use std::collections::BTreeSet;

use crate::paths::is_under;

/// Roots affected by `changed` repository-relative paths.
///
/// `shared` lists the files every root job reads (requirements, inventory):
/// a change to any selects every root.
#[must_use]
pub fn affected_roots(
    roots: &[String],
    shared: &[String],
    changed: &BTreeSet<String>,
) -> BTreeSet<String> {
    if changed.iter().any(|path| shared.contains(path)) {
        return roots.iter().cloned().collect();
    }
    roots
        .iter()
        .filter(|root| changed.iter().any(|path| is_under(path, root)))
        .cloned()
        .collect()
}
