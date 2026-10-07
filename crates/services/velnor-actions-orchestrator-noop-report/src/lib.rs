//! No-op report variant for skipped obligations.
//!
//! [`noop_report`] resolves skipped obligations against the staged
//! plan and writes validated `not_selected` reports plus downstream
//! skip reports. The hub selects this variant when the step carries
//! a no-op reason and delegates here with explicit inputs.

pub mod noop_report;
