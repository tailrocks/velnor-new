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
}
