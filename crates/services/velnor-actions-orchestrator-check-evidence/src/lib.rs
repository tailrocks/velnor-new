//! Named-check evidence verification plus the execution gate.
//!
//! Evidence side of check execution: binds producer scenario bytes to
//! the source plan and named check ([`scenario`]) and folds execution
//! receipts, container proofs, and tool proofs at the ordinary final
//! merge ([`gate`]). Builds on the acquisition and preparation leaves.

pub mod gate;
pub mod scenario;
