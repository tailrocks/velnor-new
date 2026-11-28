// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::ids::{JobId, PackageName, RepoPath, RootSlug};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Gate {
    pub slug: RootSlug,
    pub need: GateNeed,
    pub from: PlanFrom,
    /// A gate on packages passes when the plan selects any of them or the
    /// whole workspace. Empty means no package condition.
    pub packages: Vec<PackageName>,
    /// Passes exactly when the plan selects a package of the root but none
    /// of `packages`, so a notice can say why the guarded step was skipped.
    pub unselected: bool,
    pub group: Option<JobId>,
    pub also: Vec<RootSlug>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchedManifest {
    pub path: RepoPath,
    pub selected_by: Option<Gate>,
}

impl Gate {
    #[must_use]
    pub fn outputs(&self) -> Vec<(RootSlug, GateNeed)> {
        let own = |need| (self.slug.clone(), need);
        match self.need {
            GateNeed::Selected if !self.also.is_empty() => self
                .roots()
                .map(|slug| (slug.clone(), GateNeed::Selected))
                .collect(),
            GateNeed::Selected | GateNeed::Group => vec![own(GateNeed::Selected)],
            GateNeed::Doctests | GateNeed::Docs => vec![
                own(GateNeed::Selected),
                own(self.need),
                own(GateNeed::PackageArgs),
            ],
            GateNeed::Deny | GateNeed::Machete | GateNeed::Lint => vec![own(self.need)],
            GateNeed::PackageArgs if self.unselected => {
                vec![own(GateNeed::Selected), own(GateNeed::PackageArgs)]
            }
            GateNeed::Policy => vec![own(GateNeed::Deny), own(GateNeed::Machete)],
            GateNeed::TestFilter
            | GateNeed::PackageArgs
            | GateNeed::DoctestsKnown
            | GateNeed::TestsExpected => {
                vec![own(self.need)]
            }
        }
    }
}

impl Gate {
    pub fn roots(&self) -> impl Iterator<Item = &RootSlug> {
        std::iter::once(&self.slug).chain(&self.also)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum GateNeed {
    Selected,
    Doctests,
    DoctestsKnown,
    Docs,
    TestFilter,
    PackageArgs,
    Deny,
    Machete,
    Policy,
    TestsExpected,
    Group,
    Lint,
}

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
            Self::Lint => return format!("lint_{slug}"),
        };
        if self.is_global() {
            format!("rust_{name}")
        } else {
            format!("rust_{slug}_{name}")
        }
    }
}

#[must_use]
pub fn group_output_name(slug: &RootSlug, group: &JobId) -> String {
    format!("rust_{slug}_group_{group}")
}

#[must_use]
pub fn package_args_env(slug: &RootSlug) -> String {
    format!("VELNOR_PACKAGE_ARGS_{}", slug.as_str().to_ascii_uppercase())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanFrom {
    ThisJob,
    Job(JobId),
    /// The plan outputs of a job that also builds and tests: a job that needs it must not wait for it.
    WorkJob(JobId),
}
