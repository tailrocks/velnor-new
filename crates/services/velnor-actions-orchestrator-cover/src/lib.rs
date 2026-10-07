//! Merge checks 2-3: report-set partition, per-entry coverage, revalidation.
//!
//! Extracted from the orchestrator hub: [`cover`] judges report sets
//! and entry coverage over the merge-ports vocabulary with no hub
//! dependency — manifests resolve through pinned `gh`, proofs through
//! the ports contract — so this crate is a dependency-free leaf.
//! [`cover_port`] implements the merge port over that behavior; the hub
//! keeps the original `merge_internal` entrypoint.

pub mod cover;
pub mod cover_port;
