//! Event-time `merge-v1` request assembly from downloaded artifacts.
//!
//! Extracted from the orchestrator hub: assembly resolves the run key
//! only through the merge-request-ports request trait, so this crate
//! depends on the contract and never on the hub. The hub supplies the
//! port and keeps the original entrypoints.

mod merge_request;

pub use merge_request::{
    MAX_STAGED_REPORT_BYTES, assemble_merge_request, assemble_with_needs, read_staged_reports,
    write_merge_request, write_merge_request_to,
};
