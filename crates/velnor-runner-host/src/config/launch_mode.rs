// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum LaunchMode {
    #[default]
    OfficialRunner,
    RustRunner,
}

impl LaunchMode {
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "official-runner" => Some(Self::OfficialRunner),
            "rust-runner" => Some(Self::RustRunner),
            _ => None,
        }
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::OfficialRunner => "official-runner",
            Self::RustRunner => "rust-runner",
        }
    }
}
