//! Generation coordination and scheduling (Gate 0 shell).
//!
//! Owns composition, obligation selection, cache evidence, scheduling, and
//! typed process-request coordination. Must not own file parsing, YAML
//! templates, CLI parsing, or OS process details.

/// Version marker for the orchestrator shell.
pub const ORCHESTRATOR_VERSION: u32 = 0;
