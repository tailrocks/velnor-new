// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_model::{
    TESTS_BY_PACKAGE_OUTPUT, TESTS_IGNORED_OUTPUT, TESTS_LISTED_OUTPUT, TESTS_RUN_OUTPUT,
};
use velnor_rust::{PackageCounts, encode_package_counts};

#[cfg(test)]
mod list_argv;
#[path = "tests_run/listed.rs"]
mod listed;
#[path = "tests_run/status.rs"]
mod status;
#[cfg(test)]
mod statuses;
mod tap;

pub(crate) use listed::{Listing, runnable_matches};
pub use tap::SummaryTap;

const LINE_LIMIT: usize = 4096;
const RUN_FLAGS: &[&str] = &[
    "--fail-fast",
    "--hide-progress-bar",
    "--no-capture",
    "--no-fail-fast",
    "--no-input-handler",
    "--no-output-indent",
    "--no-run",
    "--nocapture",
];
const RUN_OPTIONS: &[&str] = &[
    "--cargo-message-format",
    "--failure-output",
    "--final-status-level",
    "--flaky-result",
    "--max-fail",
    "--max-progress-running",
    "--message-format",
    "--no-tests",
    "--retries",
    "--status-level",
    "--stress-count",
    "--stress-duration",
    "--success-output",
    "--test-threads",
];

/// The command an `inputs build` wrapper runs, after any `--directory` options.
fn audited(command: &[String]) -> Option<&[String]> {
    let program = command.first()?.rsplit('/').next()?;
    if program != "velnor-ci" || command.get(1..3)? != ["inputs", "build"] {
        return None;
    }
    let dashes = command.iter().position(|arg| arg == "--")?;
    command.get(dashes + 1..)
}

fn nextest_run(command: &[String]) -> Option<&[String]> {
    let command = audited(command).unwrap_or(command);
    let program = command
        .first()
        .and_then(|program| program.rsplit('/').next())
        .unwrap_or_default();
    (matches!(program, "cargo" | "cargo-nextest")
        && command.get(1).map(String::as_str) == Some("nextest")
        && command.get(2).map(String::as_str) == Some("run"))
    .then_some(command)
}

/// nextest exit codes that come after the build: 0 (all passed) and 100 (a test failed).
/// 101 is a build failure (nextest-metadata `NextestExitCode::BUILD_FAILED`).
const TEST_RUN_FAILED: i32 = 100;

/// Whether a nextest run got past building the test binaries.
#[must_use]
pub fn built(code: i32) -> bool {
    matches!(code, 0 | TEST_RUN_FAILED)
}

#[must_use]
pub fn runs_nextest(command: &[String]) -> bool {
    nextest_run(command).is_some()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestCounts {
    pub run: u64,
    pub listed: Option<u64>,
    pub ignored: Option<u64>,
    pub by_package: Option<BTreeMap<String, PackageCounts>>,
    runs: Option<BTreeMap<String, u64>>,
}

#[must_use]
pub fn output_lines(counts: TestCounts) -> String {
    let mut lines = vec![format!("{TESTS_RUN_OUTPUT}={}", counts.run)];
    if let Some(listed) = counts.listed {
        lines.push(format!("{TESTS_LISTED_OUTPUT}={listed}"));
    }
    if let Some(ignored) = counts.ignored {
        lines.push(format!("{TESTS_IGNORED_OUTPUT}={ignored}"));
    }
    if let Some(packages) = counts.by_package
        && let Some(encoded) = encode_package_counts(&packages)
    {
        lines.push(format!("{TESTS_BY_PACKAGE_OUTPUT}={encoded}"));
    }
    lines.join("\n") + "\n"
}

fn skipped(line: &str) -> Option<u64> {
    let (_, tail) = line.split_once(" run:")?;
    tail.split(',')
        .find_map(|part| part.trim().strip_suffix(" skipped"))
        .and_then(|count| count.trim().parse().ok())
}

#[must_use]
pub(crate) fn list_argv(run: &[String]) -> Option<Vec<String>> {
    let run = nextest_run(run)?;
    let mut out = Vec::with_capacity(run.len() + 2);
    out.extend(run.iter().take(2).cloned());
    out.push("list".to_owned());
    let mut index = 3;
    let mut dashed = false;
    while index < run.len() {
        let arg = &run[index];
        if !dashed && arg == "--" {
            dashed = true;
            out.push(arg.clone());
            index += 1;
            continue;
        }
        if dashed || !drop_run_only(run, &mut index) {
            out.push(arg.clone());
            index += 1;
        }
    }
    let at = out.iter().position(|arg| arg == "--").unwrap_or(out.len());
    out.insert(at, "json".to_owned());
    out.insert(at, "--message-format".to_owned());
    Some(out)
}

fn drop_run_only(run: &[String], index: &mut usize) -> bool {
    let arg = &run[*index];
    if RUN_FLAGS.contains(&arg.as_str()) {
        *index += 1;
        return true;
    }
    let equals = RUN_OPTIONS
        .iter()
        .any(|option| arg.starts_with(option) && arg.as_bytes().get(option.len()) == Some(&b'='));
    if equals {
        *index += 1;
        return true;
    }
    if !RUN_OPTIONS.contains(&arg.as_str()) {
        return false;
    }
    *index += 1;
    if run.get(*index).is_some_and(|value| !value.starts_with('-')) {
        *index += 1;
    }
    true
}

#[must_use]
pub(crate) fn apply_runnable(mut counts: TestCounts, listing: Option<Listing>) -> TestCounts {
    let Some(Listing {
        packages: matches,
        ignored,
    }) = listing
    else {
        counts.by_package = None;
        return counts;
    };
    let Some(total) = matches
        .values()
        .try_fold(0u64, |total, count| total.checked_add(*count))
    else {
        counts.by_package = None;
        return counts;
    };
    counts.listed = Some(total);
    counts.ignored = Some(ignored);
    counts.by_package = counts
        .runs
        .as_ref()
        .and_then(|runs| combine(runs, &matches))
        .filter(|packages| reconciled(packages, counts.run, total));
    counts
}

fn combine(
    runs: &BTreeMap<String, u64>,
    matches: &BTreeMap<String, u64>,
) -> Option<BTreeMap<String, PackageCounts>> {
    let mut packages = BTreeMap::new();
    for (name, listed) in matches {
        let run = runs.get(name).copied().unwrap_or(0);
        if run > *listed {
            return None;
        }
        packages.insert(
            name.clone(),
            PackageCounts {
                run,
                listed: *listed,
            },
        );
    }
    runs.iter()
        .all(|(name, run)| *run == 0 || matches.contains_key(name))
        .then_some(packages)
}

fn reconciled(packages: &BTreeMap<String, PackageCounts>, run: u64, listed: u64) -> bool {
    let summed_run = packages
        .values()
        .try_fold(0u64, |total, package| total.checked_add(package.run));
    let summed_listed = packages
        .values()
        .try_fold(0u64, |total, package| total.checked_add(package.listed));
    summed_run == Some(run) && summed_listed == Some(listed)
}
