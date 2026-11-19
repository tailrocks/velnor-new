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
            _ => Ok(()),
        }
    }
}
