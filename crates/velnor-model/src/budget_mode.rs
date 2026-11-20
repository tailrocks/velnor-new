// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::ids::{JobId, RootSlug};

/// The first `velnor-ci` release that reads the selection of a run from the
/// name of its budget job and applies the per-mode tables of `[budget]`.
pub const BUDGET_MODES_HELPER: (u64, u64, u64) = (0, 1, 104);

pub const BUDGET_JOB_NAME: &str = "Speed budget";

/// How much of the workspace the plan of a run selected. A run that tests
/// everything and a run that tests nothing differ by an order of magnitude, so
/// the speed budget measures each kind against its own runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Selection {
    /// Some root selected everything (`rust_<root>_mode=all`).
    Full,
    /// Some packages were selected and no root selected everything (`some`).
    Affected,
    /// No root selected a package (`none`).
    Nothing,
}

impl Selection {
    pub const ALL: [Self; 3] = [Self::Full, Self::Affected, Self::Nothing];

    /// The name in `[budget.<name>]` and in the budget job's title.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Affected => "affected",
            Self::Nothing => "none",
        }
    }

    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|mode| mode.name() == name)
    }

    /// The value of `rust_<root>_mode` that selects this mode.
    #[must_use]
    pub fn plan_value(self) -> &'static str {
        match self {
            Self::Full => "all",
            Self::Affected => "some",
            Self::Nothing => "none",
        }
    }

    /// The selection a budget job title records, for example
    /// `Speed budget (full)`. A title without a known selection is `None`.
    #[must_use]
    pub fn of_job_title(title: &str) -> Option<Self> {
        let inner = title
            .strip_prefix(BUDGET_JOB_NAME)?
            .strip_prefix(" (")?
            .strip_suffix(')')?;
        Self::from_name(inner)
    }
}

/// One limit per measure of a mode; `None` falls back to the plain key of
/// `[budget]`, `Some(0)` switches the check off for that mode.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ModeBudget {
    pub pr_wall_seconds: Option<u64>,
    pub main_wall_seconds: Option<u64>,
    pub runner_seconds: Option<u64>,
    pub setup_seconds: Option<u64>,
}

impl ModeBudget {
    #[must_use]
    pub fn is_set(&self) -> bool {
        *self != Self::default()
    }
}

/// The job title of the budget job: it names the selection of the run from the
/// plan outputs of `plan`, so a later run can read it from the jobs API.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectionName {
    pub plan: JobId,
    pub roots: Vec<RootSlug>,
    pub after: JobId,
}
