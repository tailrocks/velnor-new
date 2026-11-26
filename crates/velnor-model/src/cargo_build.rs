// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::plan::CargoCommand;

impl CargoCommand {
    /// Whether the command compiles the build the cache saves, as opposed to a check
    /// that reads it (lint, doc, per-package check, tests, policy). Exhaustive, so a
    /// new command must be classified when it is added.
    #[must_use]
    pub const fn is_target_build(&self) -> bool {
        match self {
            Self::BuildBinary { .. }
            | Self::BuildAll
            | Self::NextestBuild
            | Self::NextestArchive { .. } => true,
            Self::FmtCheck
            | Self::Clippy { .. }
            | Self::CheckAlone { .. }
            | Self::NextestRun { .. }
            | Self::NextestFeatures { .. }
            | Self::ClippyFeatures { .. }
            | Self::DocFeatures { .. }
            | Self::LaneGuard { .. }
            | Self::NextestRunArchive { .. }
            | Self::TestTarget { .. }
            | Self::Doctest
            | Self::Doc
            | Self::Deny { .. }
            | Self::Machete => false,
        }
    }

    /// Whether the command builds the whole workspace for the lint, the docs or the tests, so the
    /// repository's `workspace_features` apply to it. The per-package checks, the feature lanes
    /// and the policy commands never take them.
    #[must_use]
    pub const fn takes_workspace_features(&self) -> bool {
        match self {
            Self::Clippy { .. }
            | Self::Doc
            | Self::NextestBuild
            | Self::NextestArchive { .. }
            | Self::NextestRun { .. } => true,
            Self::BuildBinary { .. }
            | Self::BuildAll
            | Self::FmtCheck
            | Self::CheckAlone { .. }
            | Self::NextestFeatures { .. }
            | Self::ClippyFeatures { .. }
            | Self::DocFeatures { .. }
            | Self::LaneGuard { .. }
            | Self::NextestRunArchive { .. }
            | Self::TestTarget { .. }
            | Self::Doctest
            | Self::Deny { .. }
            | Self::Machete => false,
        }
    }

    #[must_use]
    pub const fn is_compile_check(&self) -> bool {
        match self {
            Self::Clippy { .. }
            | Self::CheckAlone { .. }
            | Self::ClippyFeatures { .. }
            | Self::DocFeatures { .. }
            | Self::Doc => true,
            Self::BuildBinary { .. }
            | Self::BuildAll
            | Self::NextestBuild
            | Self::NextestArchive { .. }
            | Self::FmtCheck
            | Self::NextestRun { .. }
            | Self::NextestFeatures { .. }
            | Self::LaneGuard { .. }
            | Self::NextestRunArchive { .. }
            | Self::TestTarget { .. }
            | Self::Doctest
            | Self::Deny { .. }
            | Self::Machete => false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildRecord {
    Unrecorded,
    Tests,
    TestsAndLanes,
}

impl BuildRecord {
    #[must_use]
    pub const fn tests(self) -> bool {
        !matches!(self, Self::Unrecorded)
    }

    #[must_use]
    pub const fn lanes(self) -> bool {
        matches!(self, Self::TestsAndLanes)
    }
}
