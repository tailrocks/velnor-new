//! Known-stack vocabulary: closed per-stack dispatch enum.
//!
//! Moved from `discover::registry` at the SIZE split: [`Stack`] is shared
//! vocabulary (config validates stack selections against it, discovery
//! dispatches on it), while detector entries stay in the planning crate.

use crate::errors::ContractError;

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
