// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};

use super::cache_family::{CARGO_HOME_EXPR, RUSTUP_HOME_EXPR};

pub const RUSTUP_AUTO_INSTALL_ENV: &str = "RUSTUP_AUTO_INSTALL";

pub const RUSTUP_AUTO_INSTALL_DISABLED: &str = "0";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum HomeKey {
    CargoHome,
    MiseCargoHome,
    MiseRustupHome,
    RustupHome,
}

impl HomeKey {
    pub const ALL: [Self; 4] = [
        Self::CargoHome,
        Self::MiseCargoHome,
        Self::MiseRustupHome,
        Self::RustupHome,
    ];

    #[must_use]
    pub const fn env_name(self) -> &'static str {
        match self {
            Self::CargoHome => "CARGO_HOME",
            Self::RustupHome => "RUSTUP_HOME",
            Self::MiseCargoHome => "MISE_CARGO_HOME",
            Self::MiseRustupHome => "MISE_RUSTUP_HOME",
        }
    }

    #[must_use]
    pub const fn standard_value(self) -> &'static str {
        match self {
            Self::CargoHome | Self::MiseCargoHome => CARGO_HOME_EXPR,
            Self::RustupHome | Self::MiseRustupHome => RUSTUP_HOME_EXPR,
        }
    }

    #[must_use]
    pub fn from_env_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|key| key.env_name() == name)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobEnv {
    #[default]
    Unset,
    RustToolchain,
}

impl JobEnv {
    #[must_use]
    pub const fn is_unset(&self) -> bool {
        matches!(self, Self::Unset)
    }

    #[must_use]
    pub fn pairs(self) -> Vec<(&'static str, &'static str)> {
        match self {
            Self::Unset => Vec::new(),
            Self::RustToolchain => HomeKey::ALL
                .into_iter()
                .map(|key| (key.env_name(), key.standard_value()))
                .chain([(RUSTUP_AUTO_INSTALL_ENV, RUSTUP_AUTO_INSTALL_DISABLED)])
                .collect(),
        }
    }

    #[must_use]
    pub fn get(self, name: &str) -> Option<&'static str> {
        self.pairs()
            .into_iter()
            .find_map(|(key, value)| (key == name).then_some(value))
    }
}
