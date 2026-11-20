// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::ids::{PackageName, RepoPath, TargetName};
use crate::plan::GateNeed;
use crate::repo::{LaneFeatures, TargetKind};
use crate::test_group::NoTests;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CargoCommand {
    FmtCheck,
    Clippy {
        extra: Vec<String>,
    },
    CheckAlone {
        package: PackageName,
    },
    BuildBinary {
        package: PackageName,
        binary: TargetName,
    },
    BuildAll,
    NextestBuild,
    NextestRun {
        filter: FilterRef,
        ci_profile: bool,
    },
    NextestFeatures {
        features: LaneFeatures,
        packages: Vec<PackageName>,
        filter: FilterRef,
        ci_profile: bool,
        reports: bool,
    },
    ClippyFeatures {
        features: LaneFeatures,
    },
    DocFeatures {
        features: LaneFeatures,
    },
    LaneGuard {
        features: LaneFeatures,
        packages: Vec<PackageName>,
        filter: String,
        except: Vec<String>,
    },
    NextestArchive {
        file: RepoPath,
    },
    NextestRunArchive {
        file: RepoPath,
        filter: FilterRef,
        threads: Option<u16>,
        partition: Option<(u16, u16)>,
        ci_profile: bool,
        no_tests: NoTests,
    },
    TestTarget {
        package: PackageName,
        target: TargetName,
        kind: TargetKind,
        features: Vec<String>,
    },
    Doctest,
    Doc,
    Deny {
        manifest: RepoPath,
        config: Option<RepoPath>,
    },
    Machete,
}

impl CargoCommand {
    #[must_use]
    pub fn builds(&self) -> bool {
        !matches!(
            self,
            Self::FmtCheck | Self::Deny { .. } | Self::Machete | Self::NextestRunArchive { .. }
        )
    }

    #[must_use]
    pub const fn reports_tests(&self) -> bool {
        matches!(
            self,
            Self::NextestRun { .. }
                | Self::NextestRunArchive { .. }
                | Self::NextestFeatures { reports: true, .. }
        )
    }

    #[must_use]
    pub fn plan_values(&self) -> &'static [GateNeed] {
        match self {
            Self::NextestRun { .. }
            | Self::NextestFeatures { .. }
            | Self::NextestRunArchive { .. } => &[GateNeed::TestFilter],
            Self::Doctest => &[GateNeed::DoctestsKnown],
            _ => &[],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FilterRef {
    All,
    AllExcept(Vec<String>),
    Expr {
        expr: String,
        except: Vec<String>,
    },
    Group {
        include: Option<String>,
        exclude: Vec<String>,
        except: Vec<String>,
    },
}
