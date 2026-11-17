// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use crate::error::CiError;
use crate::metrics::RunMetrics;

const CHANGED_JOBS_SHOWN: usize = 10;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum JobDuration {
    #[default]
    Absent,
    Unmeasured,
    Measured(u64),
}

pub fn compare(a: &RunMetrics, b: &RunMetrics) -> Result<String, CiError> {
    for (what, left, right) in [
        ("repository", &a.repo, &b.repo),
        ("workflow", &a.workflow, &b.workflow),
        ("event", &a.event, &b.event),
    ] {
        if left != right {
            return Err(CiError::new(format!(
                "cannot compare runs of different {what}s: {left} and {right}"
            )));
        }
    }
    let rows: [(&str, Option<u64>, Option<u64>); 6] = [
        ("Wall seconds", a.wall_seconds, b.wall_seconds),
        ("Runner seconds", a.runner_seconds, b.runner_seconds),
        (
            "Jobs run",
            Some(a.jobs_run() as u64),
            Some(b.jobs_run() as u64),
        ),
        (
            "Jobs skipped",
            Some(a.jobs_skipped() as u64),
            Some(b.jobs_skipped() as u64),
        ),
        (
            "Crates compiled",
            Some(a.crates_compiled()),
            Some(b.crates_compiled()),
        ),
        (
            "Crates downloaded",
            Some(a.crates_downloaded()),
            Some(b.crates_downloaded()),
        ),
    ];
    let metric_lines: Vec<String> = rows
        .iter()
        .map(|(label, left, right)| {
            format!(
                "| {label} | {} | {} | {} |",
                shown(*left),
                shown(*right),
                change(*left, *right)
            )
        })
        .collect();
    let job_lines: Vec<String> = changed_jobs(a, b)
        .into_iter()
        .map(|(name, left, right)| {
            format!(
                "| {name} | {} | {} | {} |",
                shown_job(left),
                shown_job(right),
                change_job(left, right)
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
    Ok(format!(
        "| Metric | A | B | Change |\n|---|---:|---:|---:|\n{}\n{jobs_table}",
        metric_lines.join("\n")
    ))
}

fn changed_jobs(a: &RunMetrics, b: &RunMetrics) -> Vec<(String, JobDuration, JobDuration)> {
    let mut by_key: BTreeMap<(String, usize), (JobDuration, JobDuration)> = BTreeMap::new();
    for (key, seconds) in occurrence_keys(a) {
        by_key.entry(key).or_default().0 = seconds;
    }
    for (key, seconds) in occurrence_keys(b) {
        by_key.entry(key).or_default().1 = seconds;
    }
    let mut changed: Vec<_> = by_key
        .into_iter()
        .filter(|(_, (left, right))| changed_duration(*left, *right))
        .map(|((name, occurrence), (left, right))| (job_label(&name, occurrence), left, right))
        .collect();
    changed.sort_by_key(|(_, left, right)| match (left, right) {
        (JobDuration::Unmeasured, _) | (_, JobDuration::Unmeasured) => {
            (false, std::cmp::Reverse(0))
        }
        _ => (
            true,
            std::cmp::Reverse(
                left.seconds_or_absent()
                    .unwrap_or_default()
                    .abs_diff(right.seconds_or_absent().unwrap_or_default()),
            ),
        ),
    });
    changed.truncate(CHANGED_JOBS_SHOWN);
    changed
}

fn occurrence_keys(run: &RunMetrics) -> Vec<((String, usize), JobDuration)> {
    let mut seen: BTreeMap<&str, usize> = BTreeMap::new();
    run.jobs
        .iter()
        .filter(|job| job.ran())
        .map(|job| {
            let count = seen.entry(&job.name).or_default();
            *count += 1;
            (
                (job.name.clone(), *count),
                job.duration_seconds
                    .map_or(JobDuration::Unmeasured, JobDuration::Measured),
            )
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

fn shown_job(duration: JobDuration) -> String {
    match duration {
        JobDuration::Absent => "-".to_owned(),
        JobDuration::Unmeasured => "unmeasured".to_owned(),
        JobDuration::Measured(seconds) => seconds.to_string(),
    }
}

fn change(left: Option<u64>, right: Option<u64>) -> String {
    match (left, right) {
        (Some(left), Some(right)) => signed(left, right),
        _ => "-".to_owned(),
    }
}

fn change_job(left: JobDuration, right: JobDuration) -> String {
    match (left, right) {
        (JobDuration::Measured(left), JobDuration::Measured(right)) => signed(left, right),
        (JobDuration::Measured(left), JobDuration::Absent) => signed(left, 0),
        (JobDuration::Absent, JobDuration::Measured(right)) => signed(0, right),
        (left, right) if left == right => "+0".to_owned(),
        _ => "-".to_owned(),
    }
}

fn changed_duration(left: JobDuration, right: JobDuration) -> bool {
    match (left, right) {
        (JobDuration::Measured(0) | JobDuration::Absent, JobDuration::Absent)
        | (JobDuration::Absent, JobDuration::Measured(0))
        | (JobDuration::Unmeasured, JobDuration::Unmeasured) => false,
        (left, right) => left != right,
    }
}

impl JobDuration {
    fn seconds_or_absent(self) -> Option<u64> {
        match self {
            Self::Absent => Some(0),
            Self::Unmeasured => None,
            Self::Measured(seconds) => Some(seconds),
        }
    }
}

fn signed(left: u64, right: u64) -> String {
    if right >= left {
        format!("+{}", right - left)
    } else {
        format!("-{}", left - right)
    }
}
