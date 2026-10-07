//! Plan-time baseline evidence and coverage classification.
//!
//! Extracted from the orchestrator hub: [`cover_baseline`] resolves live
//! manifests only through the merge-ports baseline trait and classifies
//! obligations through [`cover_identity`], so this crate depends on the
//! contract and never on the hub. The hub supplies the port and keeps
//! the original `apply_baseline` entrypoint.

pub mod cover_baseline;
pub mod cover_identity;
