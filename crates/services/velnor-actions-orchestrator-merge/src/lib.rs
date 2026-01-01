//! Event-time `merge-v1` JSON entrypoint (schema 1).
//!
//! Extracted from the orchestrator hub: merge calls cover behavior only
//! through the merge-ports cover trait, so this crate depends on the
//! contract and never on the hub. The hub supplies the port and keeps
//! the original `merge_internal` entrypoint.

mod merge;

pub use merge::{
    BaselineManifest, MergeRequest, merge_checks, merge_internal_with, merge_lenient,
    required_evidence,
};
