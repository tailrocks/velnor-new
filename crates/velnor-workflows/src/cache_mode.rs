// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

/// The `cache-mode` of a job: what the Actions cache service lets it do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheMode {
    Read,
    Write,
    WriteOnly,
    None,
}

impl CacheMode {
    pub const NAMES: &[&'static str] = &["read", "write", "write-only", "none"];

    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "read" => Self::Read,
            "write" => Self::Write,
            "write-only" => Self::WriteOnly,
            "none" => Self::None,
            _ => return None,
        })
    }

    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Read => Self::NAMES[0],
            Self::Write => Self::NAMES[1],
            Self::WriteOnly => Self::NAMES[2],
            Self::None => Self::NAMES[3],
        }
    }

    #[must_use]
    pub fn writes(self) -> bool {
        matches!(self, Self::Write | Self::WriteOnly)
    }
}
