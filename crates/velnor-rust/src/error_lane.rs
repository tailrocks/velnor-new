// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt;

use crate::error::Reason;

impl Reason {
    pub(crate) fn fmt_test_lane(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownTestLaneRoot(root) => write!(
                f,
                "test_lane names the root {root}, which is not a Cargo root of this repository; name the folder of a root's Cargo.toml, or . for the repository root"
            ),
            Self::TestLaneWithoutNextest(root) => write!(
                f,
                "test_lane names the root {root}, which runs no nextest tests; remove the lane or give the root a test the lane can run"
            ),
            Self::UnknownTestLanePackage(package) => write!(
                f,
                "test_lane.packages names {package}, which is not a package of the lane's root; name the packages that hold the lane's tests"
            ),
            Self::TestLaneFeatureConflict { first, second } => write!(
                f,
                "test_lane sets {first} together with {second}, which select features in conflicting ways; keep one of them: all_features selects every feature and no_default_features drops the default ones, so the two cannot be combined with each other or with a features list"
            ),
            Self::UnknownWorkspaceFeature { root, feature } => write!(
                f,
                "workspace_features names {feature}, which is not a key of the [features] table of any package of the root {root}, so cargo could refuse the workspace-wide commands of that root; a name must be a plain feature name that a package of every root defines under [features] (an optional dependency counts only once a [features] entry names it)"
            ),
            Self::TestLaneDuplicatesWorkspaceFeatures { features } => write!(
                f,
                "a test_lane builds with {features}, which workspace_features already turns on for the whole workspace, so the lane would compile the workspace a second time and run tests the main Test already runs; delete the lane"
            ),
            Self::GatedHarnessWithWorkspaceFeatures { package, target } => write!(
                f,
                "workspace_features is set, but the harness = false target {target} of {package} has required-features, and the generator cannot tell whether cargo builds it with those features; remove workspace_features and keep a [[test_lane]] for this repository"
            ),
            _ => Ok(()),
        }
    }
}
