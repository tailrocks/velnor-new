//! actionlint pin/capability metadata and config (Gate 0 shell).
//!
//! Owns generated config and action-schema validation. Must not own Mise
//! process execution, stack scanning, or generic workflow rendering.

/// Stable identifier for the actionlint tool metadata.
pub const TOOL_ID: &str = "actionlint";
