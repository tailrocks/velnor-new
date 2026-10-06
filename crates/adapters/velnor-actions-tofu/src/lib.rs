//! `OpenTofu` stack adapter: closure resolution, task identity, and
//! calling-root selection over the [`velnor_actions_tofu_core`]
//! foundation (detection, parsing, evidence, proposals).
//!
//! This crate launches no processes, builds no tool invocations,
//! and renders no workflow text.

pub mod closure;
pub mod closure_inputs;
pub mod identity;
pub mod select;

pub use closure::resolve_closure_at_root;
pub use identity::{
    TofuGroupExtensionInputs, entry_metadata_for_task, extension_for_proposal, lock_slot_at_root,
};
pub use select::{RootSelection, SelectAllReason, select_roots};
