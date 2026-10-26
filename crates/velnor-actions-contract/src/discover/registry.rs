// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::discover::FileIndex;
use crate::errors::ContractError;
use crate::propose::StackCandidate;

pub type DetectorEntry = (&'static str, u32, fn(&FileIndex) -> Vec<StackCandidate>);

pub const DETECTION_SCHEMA: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Stack {
    Native,

    Mise,

    Rust,

    Tofu,
}

impl Stack {
    #[must_use]
    pub fn all() -> &'static [Self] {
        &[Self::Mise, Self::Native, Self::Rust, Self::Tofu]
    }

    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::Native => "native",
            Self::Mise => "mise",
            Self::Rust => "rust",
            Self::Tofu => "tofu",
        }
    }

    #[must_use]
    pub const fn is_ignorable(self) -> bool {
        match self {
            Self::Native | Self::Mise => false,
            Self::Rust | Self::Tofu => true,
        }
    }

    #[must_use]
    pub fn from_id(id: &str) -> Option<Self> {
        Self::all().iter().find(|stack| stack.id() == id).copied()
    }

    pub fn require_known(id: &str) -> Result<Self, ContractError> {
        Self::from_id(id).ok_or_else(|| ContractError::identity("stack_id", "unregistered_stack"))
    }
}
