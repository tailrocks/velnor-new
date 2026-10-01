//! `OpenTofu` stack adapter: registration, roots, and dialect evidence.
//!
//! T09 owns strict minimal `[stacks.tofu]` roots plus STRONG / WEAK /
//! CONFLICT evidence. Candidates come from configured roots via
//! [`qualify_roots`]; the `fn(&FileIndex)` detector entry stays empty
//! because file markers alone never suffice (WEAK never claims;
//! STRONG without a table only advises). Task inventory, identity,
//! and closure arms stay fail-closed for T10+. This crate launches
//! no processes, builds no tool invocations, and renders no workflow
//! text.

pub mod detect;
pub mod effective;
pub mod evidence;
pub mod roots;

pub use detect::discover_stack_candidates;
pub use effective::{Dialect, config_shape, dir_has_effective_config, effective_set};
pub use evidence::{
    Advisory, Evidence, EvidenceLevel, MISE_OPENTOFU_TOOL, MISE_TERRAFORM_TOOL, TofuNote, classify,
    mise_tool_selected, plan_note,
};
pub use roots::qualify_roots;

/// Stable identifier for the `OpenTofu` stack.
pub const STACK_ID: &str = "tofu";
