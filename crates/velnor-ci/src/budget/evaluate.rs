// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::SpeedBudget;

use super::{Breach, Evaluation, Group, History, JobDelta, Measure, Medians, Source, median};

pub(super) fn measure_kinds(
    budget: &SpeedBudget,
    histories: &[History],
    evaluation: &mut Evaluation,
) {
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
    if budget.limits_pull_request_wall() && pull_requests == 0 {
        evaluation
            .missing
            .push("the pull request runs are not measured".to_owned());
    }
    if budget.limits_main_wall() && default_branch == 0 {
        evaluation
            .missing
            .push("the default branch runs are not measured".to_owned());
    }
}

pub(super) fn measure_population(
    budget: &SpeedBudget,
    history: &History,
    evaluation: &mut Evaluation,
) {
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
    let group = Group {
        population: history.population,
        mode: history.mode,
    };
    let limits = budget.for_mode(history.mode);
    evaluation.medians.push(Medians {
        population: history.population,
        mode: history.mode,
        samples: window,
        wall_seconds: wall.unwrap_or_default(),
        runner_seconds: runner.unwrap_or_default(),
        setup_seconds: setup,
    });
    compare(
        evaluation,
        group,
        Measure::Wall,
        limits.wall_seconds(history.population.is_pull_request()),
        Some(latest.wall_seconds),
        wall,
        window > 1,
    );
    compare(
        evaluation,
        group,
        Measure::Runner,
        limits.runner_seconds,
        Some(latest.runner_seconds),
        runner,
        window > 1,
    );
    compare(
        evaluation,
        group,
        Measure::Setup,
        limits.setup_seconds,
        latest.setup_seconds,
        setup,
        window > 1,
    );
    evaluation.slowest_jobs.extend(job_deltas(history));
}

fn compare(
    evaluation: &mut Evaluation,
    group: Group,
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
            population: group.population,
            mode: group.mode,
            measure,
            source: Source::Latest,
            value: latest,
            threshold,
        }),
        Some(_) => {}
        None => evaluation.missing.push(format!(
            "the {} {} of the latest run is not measured",
            group.name(),
            measure.name()
        )),
    }
    if !compare_median {
        return;
    }
    match median {
        Some(median) if median > threshold => evaluation.breaches.push(Breach {
            population: group.population,
            mode: group.mode,
            measure,
            source: Source::Median,
            value: median,
            threshold,
        }),
        Some(_) => {}
        None => evaluation.missing.push(format!(
            "the {} {} of the measured runs is not measured",
            group.name(),
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
                mode: history.mode,
                name: name.clone(),
                seconds: *seconds,
                median,
            });
        }
    }
    deltas
}
