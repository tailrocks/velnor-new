// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{ModeBudget, Selection, SpeedBudget};

use crate::budget::Evaluation;

fn limit(seconds: u64) -> String {
    if seconds == 0 {
        "off".to_owned()
    } else {
        format!("{seconds}s")
    }
}

fn plain_text(budget: &SpeedBudget) -> String {
    [
        ("pull request wall seconds", budget.pr_wall_seconds),
        ("default branch wall seconds", budget.main_wall_seconds),
        ("runner seconds", budget.runner_seconds),
        ("setup seconds", budget.setup_seconds),
    ]
    .map(|(name, seconds)| {
        if seconds == 0 {
            format!("{name} off")
        } else {
            format!("{name} {seconds}s")
        }
    })
    .join(", ")
}

fn mode_text(mode: &ModeBudget) -> String {
    [
        ("pull request wall seconds", mode.pr_wall_seconds),
        ("default branch wall seconds", mode.main_wall_seconds),
        ("runner seconds", mode.runner_seconds),
        ("setup seconds", mode.setup_seconds),
    ]
    .into_iter()
    .filter_map(|(name, seconds)| seconds.map(|seconds| format!("{name} {}", limit(seconds))))
    .collect::<Vec<_>>()
    .join(", ")
}

#[must_use]
pub fn budget_text(budget: &SpeedBudget) -> String {
    let modes = [
        (Selection::Full, &budget.full),
        (Selection::Affected, &budget.affected),
        (Selection::Nothing, &budget.none),
    ]
    .into_iter()
    .filter(|(_, table)| table.is_set())
    .map(|(mode, table)| format!("{} selection: {}", mode.name(), mode_text(table)));
    std::iter::once(plain_text(budget))
        .chain(modes)
        .collect::<Vec<_>>()
        .join("; ")
}

/// The wall time and runner seconds of the running run next to the limits of
/// its own selection.
#[must_use]
pub fn current_report(
    budget: &SpeedBudget,
    mode: Option<Selection>,
    wall_seconds: u64,
    runner_seconds: u64,
) -> String {
    let limits = budget.for_mode(mode);
    let kind = mode.map_or_else(String::new, |mode| format!(" {} selection", mode.name()));
    format!(
        "this run so far: wall {wall_seconds}s (default branch{kind} budget {}), runner {runner_seconds}s (budget {})\n",
        limit(limits.main_wall_seconds),
        limit(limits.runner_seconds)
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
            medians.group().name(),
            medians.samples,
            medians.wall_seconds,
            medians.runner_seconds
        ));
    }
    for breach in &evaluation.breaches {
        lines.push(format!(
            "breach: the {} {} of the {} is {}s, above the budget {}s",
            breach.group().name(),
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
            job.group().name(),
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
