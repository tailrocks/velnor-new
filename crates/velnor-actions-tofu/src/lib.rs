//! `OpenTofu` stack registration: stack id plus detector entry.
//!
//! Registration only (T08): the detector emits no candidates until the
//! T09 roots work lands. This crate launches no processes, builds no
//! tool invocations, reads no files itself, and renders no workflow
//! text.

pub mod detect;

pub use detect::discover_stack_candidates;

/// Stable identifier for the `OpenTofu` stack.
pub const STACK_ID: &str = "tofu";
