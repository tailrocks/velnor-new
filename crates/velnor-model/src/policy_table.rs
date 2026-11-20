// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PolicyTable {
    pub deny: DenyLane,
    pub deny_checks: Option<Vec<DenyCheck>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DenyLane {
    #[default]
    Auto,
    On,
    Off,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DenyCheck {
    Advisories,
    Bans,
    Licenses,
    Sources,
}

impl DenyCheck {
    pub const ALL: [Self; 4] = [Self::Advisories, Self::Bans, Self::Licenses, Self::Sources];

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Advisories => "advisories",
            Self::Bans => "bans",
            Self::Licenses => "licenses",
            Self::Sources => "sources",
        }
    }
}
