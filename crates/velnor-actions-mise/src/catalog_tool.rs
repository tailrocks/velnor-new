// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::MiseError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PinnedTool {
    Rust,
    MrBoxington,
    Gh,
    Jactionlint,
    Shellcheck,
    Nextest,
    Opentofu,
    ReleasePlz,
    Crane,
    Bun,
    Cosign,
}

impl PinnedTool {
    pub const ALL: [Self; 11] = [
        Self::Rust,
        Self::MrBoxington,
        Self::Gh,
        Self::Jactionlint,
        Self::Shellcheck,
        Self::Nextest,
        Self::Opentofu,
        Self::ReleasePlz,
        Self::Crane,
        Self::Bun,
        Self::Cosign,
    ];

    #[must_use]
    pub const fn tool_name(self) -> &'static str {
        match self {
            Self::Rust => "rust",
            Self::MrBoxington => "mr-boxington",
            Self::Gh => "gh",
            Self::Jactionlint => "jactionlint",
            Self::Shellcheck => "shellcheck",
            Self::Nextest => "nextest",
            Self::Opentofu => "opentofu",
            Self::ReleasePlz => "release-plz",
            Self::Crane => "crane",
            Self::Bun => "bun",
            Self::Cosign => "cosign",
        }
    }

    pub fn from_tool_name(name: &str) -> Result<Self, MiseError> {
        Self::ALL
            .iter()
            .find(|tool| tool.tool_name() == name)
            .copied()
            .ok_or_else(|| MiseError::UnknownTool {
                tool: name.to_owned(),
            })
    }
}
