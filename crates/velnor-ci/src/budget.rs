// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{Selection, SpeedBudget};

mod evaluate;
use evaluate::{measure_kinds, measure_population};

pub use crate::budget_report::{budget_text, current_report, report};

pub const RUNS_USED: usize = 10;
pub const SLOWEST_JOBS: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Population {
    PullRequest,
    MainPush,
    Schedule,
}

impl Population {
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::PullRequest => "pull request",
            Self::MainPush => "default branch",
            Self::Schedule => "schedule",
        }
    }

    #[must_use]
    pub fn is_pull_request(self) -> bool {
        matches!(self, Self::PullRequest)
    }
}

/// A kind of run measured against its own history: the event and, when the
/// workflow records it, what the plan selected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Group {
    pub population: Population,
    pub mode: Option<Selection>,
}

impl Group {
    #[must_use]
    pub fn name(self) -> String {
        match self.mode {
            Some(mode) => format!("{} {} selection", self.population.name(), mode.name()),
            None => self.population.name().to_owned(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sample {
    pub run_id: u64,
    /// What the plan of the run selected; `None` when the run does not say.
    pub selection: Option<Selection>,
    pub wall_seconds: u64,
    pub runner_seconds: u64,
    pub setup_seconds: Option<u64>,
    pub jobs: Vec<(String, u64)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct History {
    pub population: Population,
    pub mode: Option<Selection>,
    pub samples: Vec<Sample>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Measure {
    Wall,
    Runner,
    Setup,
}

impl Measure {
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Wall => "wall seconds",
            Self::Runner => "runner seconds",
            Self::Setup => "setup seconds",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Latest,
    Median,
}

impl Source {
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Latest => "latest run",
            Self::Median => "median of the window",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Breach {
    pub population: Population,
    pub mode: Option<Selection>,
    pub measure: Measure,
    pub source: Source,
    pub value: u64,
    pub threshold: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Medians {
    pub population: Population,
    pub mode: Option<Selection>,
    pub samples: usize,
    pub wall_seconds: u64,
    pub runner_seconds: u64,
    pub setup_seconds: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobDelta {
    pub population: Population,
    pub mode: Option<Selection>,
    pub name: String,
    pub seconds: u64,
    pub median: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Evaluation {
    pub breaches: Vec<Breach>,
    pub missing: Vec<String>,
    pub medians: Vec<Medians>,
    pub slowest_jobs: Vec<JobDelta>,
}

impl Breach {
    #[must_use]
    pub fn group(&self) -> Group {
        Group {
            population: self.population,
            mode: self.mode,
        }
    }
}

impl Medians {
    #[must_use]
    pub fn group(&self) -> Group {
        Group {
            population: self.population,
            mode: self.mode,
        }
    }
}

impl JobDelta {
    #[must_use]
    pub fn group(&self) -> Group {
        Group {
            population: self.population,
            mode: self.mode,
        }
    }
}

impl Evaluation {
    #[must_use]
    pub fn passed(&self) -> bool {
        self.breaches.is_empty() && self.missing.is_empty()
    }
}

#[must_use]
pub fn evaluate(budget: &SpeedBudget, histories: &[History]) -> Evaluation {
    let mut evaluation = Evaluation {
        breaches: Vec::new(),
        missing: Vec::new(),
        medians: Vec::new(),
        slowest_jobs: Vec::new(),
    };
    measure_kinds(budget, histories, &mut evaluation);
    for history in histories {
        measure_population(budget, history, &mut evaluation);
    }
    evaluation.slowest_jobs.sort_by(|left, right| {
        right
            .seconds
            .abs_diff(right.median)
            .cmp(&left.seconds.abs_diff(left.median))
            .then_with(|| left.name.cmp(&right.name))
    });
    evaluation.slowest_jobs.truncate(SLOWEST_JOBS);
    evaluation
}

#[must_use]
pub fn median(values: impl IntoIterator<Item = u64>) -> Option<u64> {
    let mut values: Vec<u64> = values.into_iter().collect();
    if values.is_empty() {
        return None;
    }
    values.sort_unstable();
    let middle = values.len() / 2;
    Some(match values.len() % 2 {
        1 => values[middle],
        _ => values[middle - 1] + values[middle].abs_diff(values[middle - 1]).div_ceil(2),
    })
}
