//! Checkout digest slots: lockfile and Nextest-config provenance.
//!
//! Thin dispatch over the rust adapter's checkout probes
//! ([`velnor_actions_rust::lock_digest_at_root`],
//! [`velnor_actions_rust::nextest_digest_at_root`]): Cargo filenames and
//! walk-up rules live in the adapter, never here. Declared via `#[path]`
//! from `internal_plan.rs` (no `lib.rs` edit).

use std::path::Path;

use velnor_actions_rust::tasks::DigestSlot;

/// Lockfile slot at `root`: content, proven absence, or unknown.
pub(crate) fn lock_digest_at_root(root: &Path, manifest: &str) -> DigestSlot {
    velnor_actions_rust::lock_digest_at_root(root, manifest)
}

/// Nextest-config slot at `root`: content, proven absence, or unknown.
pub(crate) fn nextest_digest_at_root(root: &Path, profile_config: Option<&str>) -> DigestSlot {
    velnor_actions_rust::nextest_digest_at_root(root, profile_config)
}
