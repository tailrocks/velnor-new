// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use crate::metrics::RunMetrics;

const CHANGED_JOBS_SHOWN: usize = 10;

#[must_use]
pub fn compare(a: &RunMetrics, b: &RunMetrics) -> String {
    let rows: [(&str, u64, u64); 6] = [
        ("Wall seconds", a.wall_seconds, b.wall_seconds),
        ("Runner seconds", a.runner_seconds, b.runner_seconds),
        ("Jobs run", a.jobs_run() as u64, b.jobs_run() as u64),
        (
            "Jobs skipped",
            a.jobs_skipped() as u64,
            b.jobs_skipped() as u64,
        ),
        ("Crates compiled", a.crates_compiled(), b.crates_compiled()),
        (
            "Crates downloaded",
            a.crates_downloaded(),
            b.crates_downloaded(),
        ),
    ];
    let metric_lines: Vec<String> = rows
        .iter()
        .map(|(label, left, right)| {
            format!("| {label} | {left} | {right} | {} |", signed(*left, *right))
        })
        .collect();
    let job_lines: Vec<String> = changed_jobs(a, b)
        .into_iter()
        .map(|(name, left, right)| {
            format!(
                "| {name} | {} | {} | {} |",
                shown(left),
                shown(right),
                signed(left.unwrap_or(0), right.unwrap_or(0))
            )
        })
        .collect();
    let jobs_table = if job_lines.is_empty() {
        String::new()
    } else {
        format!(
            "\n| Job | A seconds | B seconds | Change |\n|---|---:|---:|---:|\n{}\n",
            job_lines.join("\n")
        )
    };
    format!(
        "| Metric | A | B | Change |\n|---|---:|---:|---:|\n{}\n{jobs_table}",
        metric_lines.join("\n")
    )
}

fn changed_jobs(a: &RunMetrics, b: &RunMetrics) -> Vec<(String, Option<u64>, Option<u64>)> {
    let mut by_key: BTreeMap<(String, usize), (Option<u64>, Option<u64>)> = BTreeMap::new();
    for (key, seconds) in occurrence_keys(a) {
        by_key.entry(key).or_default().0 = Some(seconds);
    }
    for (key, seconds) in occurrence_keys(b) {
        by_key.entry(key).or_default().1 = Some(seconds);
    }
    let mut changed: Vec<_> = by_key
        .into_iter()
        .filter(|(_, (left, right))| left.unwrap_or(0) != right.unwrap_or(0))
        .map(|((name, occurrence), (left, right))| (job_label(&name, occurrence), left, right))
        .collect();
    changed.sort_by_key(|(_, left, right)| {
        std::cmp::Reverse(left.unwrap_or(0).abs_diff(right.unwrap_or(0)))
    });
    changed.truncate(CHANGED_JOBS_SHOWN);
    changed
}

fn occurrence_keys(run: &RunMetrics) -> Vec<((String, usize), u64)> {
    let mut seen: BTreeMap<&str, usize> = BTreeMap::new();
    run.jobs
        .iter()
        .map(|job| {
            let count = seen.entry(&job.name).or_default();
            *count += 1;
            ((job.name.clone(), *count), job.duration_seconds)
        })
        .collect()
}

fn job_label(name: &str, occurrence: usize) -> String {
    let name = name.replace('|', "\\|");
    if occurrence == 1 {
        name
    } else {
        format!("{name} (occurrence {occurrence})")
    }
}

fn shown(value: Option<u64>) -> String {
    value.map_or_else(|| "-".to_owned(), |v| v.to_string())
}

fn signed(left: u64, right: u64) -> String {
    if right >= left {
        format!("+{}", right - left)
    } else {
        format!("-{}", left - right)
    }
}
