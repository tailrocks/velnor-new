// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::ids::RootSlug;
use crate::plan::GateNeed;

impl GateNeed {
    #[must_use]
    pub fn is_global(self) -> bool {
        matches!(self, Self::Deny | Self::Machete | Self::Policy)
    }

    #[must_use]
    pub fn output_name(self, slug: &RootSlug) -> String {
        let name = match self {
            Self::Selected => "mode",
            Self::Doctests => "doctests",
            Self::DoctestsKnown => "doctests_known",
            Self::Docs => "docs",
            Self::TestFilter => "test_filter",
            Self::PackageArgs => "package_args",
            Self::Deny => "deny",
            Self::Machete => "machete",
            Self::Policy => "policy",
            Self::TestsExpected => "tests",
            Self::Group => "group",
        };
        if self.is_global() {
            format!("rust_{name}")
        } else {
            format!("rust_{slug}_{name}")
        }
    }
}
