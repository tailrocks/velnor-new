// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReleaseAuthentication {
    #[default]
    TrustedPublishing,

    BootstrapToken,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReleaseRegistry {
    CratesIo,

    #[default]
    GitTag,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "bool", into = "bool")]
pub enum OidcQualification {
    #[default]
    Disabled,
    Enabled,
}

impl OidcQualification {
    #[must_use]
    pub const fn is_enabled(self) -> bool {
        matches!(self, Self::Enabled)
    }
}

impl std::ops::Not for &OidcQualification {
    type Output = bool;

    fn not(self) -> Self::Output {
        !self.is_enabled()
    }
}

impl From<bool> for OidcQualification {
    fn from(enabled: bool) -> Self {
        if enabled {
            Self::Enabled
        } else {
            Self::Disabled
        }
    }
}

impl From<OidcQualification> for bool {
    fn from(value: OidcQualification) -> Self {
        value.is_enabled()
    }
}
