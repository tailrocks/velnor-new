//! Checkout digest slots: lockfile and Nextest-config provenance.
//!
//! Split from [`super::closure`] (size gate): this module maps checkout
//! probes to identity digest slots, preserving proven absence
//! distinctly from ignorance. Declared via `#[path]` from
//! `internal_plan.rs` (no `lib.rs` edit).

use std::path::Path;

use velnor_actions_rust::tasks::DigestSlot;

use super::closure::{Provenance, probe_lockfile, probe_nextest_config};

/// Digest slot of a [`Provenance`], preserving absence distinctly.
///
/// Externally guarded inputs have no producer today; if one ever
/// appears it maps to unknown (fail-closed: unverified by us).
fn digest_slot(provenance: Provenance) -> DigestSlot {
    match provenance {
        Provenance::Known { digest } => DigestSlot::Known(digest),
        Provenance::AbsentProven { evidence } => DigestSlot::AbsentProven(evidence),
        Provenance::Unknown { reason } => DigestSlot::Unknown(reason),
        Provenance::GuardedExternally { guard } => {
            DigestSlot::Unknown(format!("guarded_externally:{guard}"))
        }
    }
}

/// Lockfile slot at `root`: content, proven absence, or unknown.
pub(crate) fn lock_digest_at_root(root: &Path, manifest: &str) -> DigestSlot {
    digest_slot(probe_lockfile(root, manifest))
}

/// Nextest-config slot at `root`: content, proven absence, or unknown.
pub(crate) fn nextest_digest_at_root(root: &Path, profile_config: Option<&str>) -> DigestSlot {
    digest_slot(probe_nextest_config(root, profile_config))
}
