//! Detector registry contract: entry shape, schema, known stacks.
//!
//! The [`DetectorEntry`] type plus [`DETECTION_SCHEMA`] live here; the
//! detector array itself stays in the composition root (it holds adapter
//! function pointers, which would invert the dependency DAG from here).
//! [`Stack`] is the closed per-stack dispatch enum: orchestrator dispatch
//! matches on it, never on stack-id spellings.

use crate::discover::FileIndex;
use crate::errors::ContractError;
use crate::propose::StackCandidate;

/// Detector registry entry: stack id, record schema, implementation.
pub type DetectorEntry = (&'static str, u32, fn(&FileIndex) -> Vec<StackCandidate>);

/// Detector record schema: `{ stack_id, project_root, manifest }`.
pub const DETECTION_SCHEMA: u32 = 1;

/// Known stacks for closed per-stack dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stack {
    /// Explicit repository-owned Mise checks (`mise`).
    Mise,
    /// Rust/Cargo stack (`rust`).
    Rust,
    /// `OpenTofu` stack (`tofu`).
    Tofu,
}

impl Stack {
    /// Every known stack, in registry (ascending id) order.
    #[must_use]
    pub fn all() -> &'static [Self] {
        &[Self::Mise, Self::Rust, Self::Tofu]
    }

    /// Registered stack id for this stack.
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::Mise => "mise",
            Self::Rust => "rust",
            Self::Tofu => "tofu",
        }
    }

    /// Whether detector-based stack selection can suppress this stack.
    /// Explicit Mise check declarations remain mandatory once configured.
    #[must_use]
    pub const fn is_ignorable(self) -> bool {
        match self {
            Self::Mise => false,
            Self::Rust | Self::Tofu => true,
        }
    }

    /// Known stack for a registry id, or `None` when unknown.
    #[must_use]
    pub fn from_id(id: &str) -> Option<Self> {
        Self::all().iter().find(|stack| stack.id() == id).copied()
    }

    /// Known stack for a registry id, failing closed when unknown.
    ///
    /// # Errors
    ///
    /// Returns `unregistered_stack` for ids outside the registry.
    pub fn require_known(id: &str) -> Result<Self, ContractError> {
        Self::from_id(id).ok_or_else(|| ContractError::identity("stack_id", "unregistered_stack"))
    }
}

#[cfg(test)]
mod tests;
