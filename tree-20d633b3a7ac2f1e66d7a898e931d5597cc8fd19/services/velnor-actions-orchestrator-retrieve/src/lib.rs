//! Event-time retrieval: exact artifact download plus baseline fetch.
//!
//! Extracted from the orchestrator hub: [`retrieve_reports`] downloads
//! each plan-expected artifact by exact derived name and
//! [`retrieve_baseline`] re-derives the plan's exact baseline beside
//! them, both with no hub dependency — the run key and repository
//! arrive as inputs and the staged filename is a private
//! value-matched constant — so this crate is a dependency-free leaf.
//! The hub keeps the environment-driven `retrieve_reports`
//! entrypoint.

pub mod retrieve_baseline;
pub mod retrieve_reports;
