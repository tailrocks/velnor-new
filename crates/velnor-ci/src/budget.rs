// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::SpeedBudget;

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sample {
    pub run_id: u64,
    pub wall_seconds: u64,
    pub runner_seconds: u64,
    pub setup_seconds: Option<u64>,
    pub jobs: Vec<(String, u64)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct History {
    pub population: Population,
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
    pub measure: Measure,
    pub source: Source,
    pub value: u64,
    pub threshold: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Medians {
    pub population: Population,
    pub samples: usize,
    pub wall_seconds: u64,
    pub runner_seconds: u64,
    pub setup_seconds: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobDelta {
    pub population: Population,
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

fn measure_kinds(budget: &SpeedBudget, histories: &[History], evaluation: &mut Evaluation) {
    let measured: usize = histories.iter().map(|history| history.samples.len()).sum();
    let pull_requests: usize = histories
        .iter()
        .filter(|history| history.population.is_pull_request())
        .map(|history| history.samples.len())
        .sum();
    let default_branch: usize = histories
        .iter()
        .filter(|history| !history.population.is_pull_request())
        .map(|history| history.samples.len())
        .sum();
    if budget.enabled() && measured == 0 {
        evaluation
            .missing
            .push("the generated workflow has no successful run to measure".to_owned());
        return;
    }
    if budget.pr_wall_seconds > 0 && pull_requests == 0 {
        evaluation
            .missing
            .push("the pull request runs are not measured".to_owned());
    }
    if budget.main_wall_seconds > 0 && default_branch == 0 {
        evaluation
            .missing
            .push("the default branch runs are not measured".to_owned());
    }
}

fn measure_population(budget: &SpeedBudget, history: &History, evaluation: &mut Evaluation) {
    let Some(latest) = history.samples.first() else {
        return;
    };
    let window = history.samples.len();
    let wall = median(history.samples.iter().map(|sample| sample.wall_seconds));
    let runner = median(history.samples.iter().map(|sample| sample.runner_seconds));
    // A run without a setup measurement leaves the window incomplete, so the
    // median is missing instead of being computed from fewer runs.
    let setup = history
        .samples
        .iter()
        .map(|sample| sample.setup_seconds)
        .collect::<Option<Vec<u64>>>()
        .and_then(median);
    evaluation.medians.push(Medians {
        population: history.population,
        samples: window,
        wall_seconds: wall.unwrap_or_default(),
        runner_seconds: runner.unwrap_or_default(),
        setup_seconds: setup,
    });
    let wall_budget = budget.wall_seconds(history.population.is_pull_request());
    compare(
        evaluation,
        history.population,
        Measure::Wall,
        wall_budget,
        Some(latest.wall_seconds),
        wall,
        window > 1,
    );
    compare(
        evaluation,
        history.population,
        Measure::Runner,
        budget.runner_seconds,
        Some(latest.runner_seconds),
        runner,
        window > 1,
    );
    compare(
        evaluation,
        history.population,
        Measure::Setup,
        budget.setup_seconds,
        latest.setup_seconds,
        setup,
        window > 1,
    );
    evaluation.slowest_jobs.extend(job_deltas(history));
}

fn compare(
    evaluation: &mut Evaluation,
    population: Population,
    measure: Measure,
    threshold: u64,
    latest: Option<u64>,
    median: Option<u64>,
    compare_median: bool,
) {
    if threshold == 0 {
        return;
    }
    match latest {
        Some(latest) if latest > threshold => evaluation.breaches.push(Breach {
            population,
            measure,
            source: Source::Latest,
            value: latest,
            threshold,
        }),
        Some(_) => {}
        None => evaluation.missing.push(format!(
            "the {} {} of the latest run is not measured",
            population.name(),
            measure.name()
        )),
    }
    if !compare_median {
        return;
    }
    match median {
        Some(median) if median > threshold => evaluation.breaches.push(Breach {
            population,
            measure,
            source: Source::Median,
            value: median,
            threshold,
        }),
        Some(_) => {}
        None => evaluation.missing.push(format!(
            "the {} {} of the measured runs is not measured",
            population.name(),
            measure.name()
        )),
    }
}

fn job_deltas(history: &History) -> Vec<JobDelta> {
    let Some(latest) = history.samples.first() else {
        return Vec::new();
    };
    let older = &history.samples[1..];
    let mut deltas = Vec::new();
    for (name, seconds) in &latest.jobs {
        let values: Vec<u64> = older
            .iter()
            .filter_map(|sample| {
                sample
                    .jobs
                    .iter()
                    .find(|(job, _)| job == name)
                    .map(|(_, seconds)| *seconds)
            })
            .collect();
        let Some(median) = median(values) else {
            continue;
        };
        if *seconds > median {
            deltas.push(JobDelta {
                population: history.population,
                name: name.clone(),
                seconds: *seconds,
                median,
            });
        }
    }
    deltas
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

#[must_use]
pub fn budget_text(budget: &SpeedBudget) -> String {
    let mut parts = Vec::new();
    for (name, seconds) in [
        ("pull request wall seconds", budget.pr_wall_seconds),
        ("default branch wall seconds", budget.main_wall_seconds),
        ("runner seconds", budget.runner_seconds),
        ("setup seconds", budget.setup_seconds),
    ] {
        parts.push(if seconds == 0 {
            format!("{name} off")
        } else {
            format!("{name} {seconds}s")
        });
    }
    parts.join(", ")
}

#[must_use]
pub fn current_report(budget: &SpeedBudget, wall_seconds: u64, runner_seconds: u64) -> String {
    let limit = |seconds: u64| {
        if seconds == 0 {
            "off".to_owned()
        } else {
            format!("{seconds}s")
        }
    };
    format!(
        "this run so far: wall {wall_seconds}s (default branch budget {}), runner {runner_seconds}s (budget {})\n",
        limit(budget.main_wall_seconds),
        limit(budget.runner_seconds)
    )
}

#[must_use]
pub fn report(budget: &SpeedBudget, evaluation: &Evaluation) -> String {
    let mut lines = vec![format!("speed budget: {}", budget_text(budget))];
    for medians in &evaluation.medians {
        let setup = medians
            .setup_seconds
            .map_or_else(|| "unmeasured".to_owned(), |seconds| format!("{seconds}s"));
        lines.push(format!(
            "{}: {} runs, median wall {}s, runner {}s, setup {setup}",
            medians.population.name(),
            medians.samples,
            medians.wall_seconds,
            medians.runner_seconds
        ));
    }
    for breach in &evaluation.breaches {
        lines.push(format!(
            "breach: the {} {} of the {} is {}s, above the budget {}s",
            breach.population.name(),
            breach.measure.name(),
            breach.source.name(),
            breach.value,
            breach.threshold
        ));
    }
    for missing in &evaluation.missing {
        lines.push(format!("missing: {missing}"));
    }
    lines.push("slowest jobs:".to_owned());
    for job in &evaluation.slowest_jobs {
        lines.push(format!(
            "  {} ({}): {}s, median {}s, +{}s",
            job.name,
            job.population.name(),
            job.seconds,
            job.median,
            job.seconds.abs_diff(job.median)
        ));
    }
    if evaluation.slowest_jobs.is_empty() {
        lines.push("  none".to_owned());
    }
    lines.join("\n") + "\n"
}
