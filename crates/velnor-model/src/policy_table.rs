// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

/// The `[policy]` table of `.velnor/rust-ci.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PolicyTable {
    pub deny: DenyLane,
    /// The cargo-deny checks to run, in the fixed order of `DenyCheck`; `None` runs all of them.
    pub deny_checks: Option<Vec<DenyCheck>>,
}

/// Whether the cargo-deny lane is on: `Auto` runs it when a `deny.toml` exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DenyLane {
    #[default]
    Auto,
    On,
    Off,
}

/// One check of `cargo deny check`. The derived order is the order the command lists them in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DenyCheck {
    Advisories,
    Bans,
    Licenses,
    Sources,
}

impl DenyCheck {
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
