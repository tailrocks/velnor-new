// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use serde::Deserialize;
use velnor_model::{TESTS_BY_PACKAGE_OUTPUT, TESTS_LISTED_OUTPUT, TESTS_RUN_OUTPUT};
use velnor_rust::{PackageCounts, encode_package_counts, package_token};

use crate::logscan::{nextest_summary, strip_ansi};

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

#[must_use]
pub fn runs_nextest(command: &[String]) -> bool {
    let program = command
        .first()
        .and_then(|program| program.rsplit('/').next())
        .unwrap_or_default();
    matches!(program, "cargo" | "cargo-nextest")
        && command.get(1).map(String::as_str) == Some("nextest")
        && command.get(2).map(String::as_str) == Some("run")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestCounts {
    pub run: u64,
    pub listed: Option<u64>,
    pub by_package: Option<BTreeMap<String, PackageCounts>>,
    runs: Option<BTreeMap<String, u64>>,
}

#[must_use]
pub fn output_lines(counts: TestCounts) -> String {
    let mut lines = vec![format!("{TESTS_RUN_OUTPUT}={}", counts.run)];
    if let Some(listed) = counts.listed {
        lines.push(format!("{TESTS_LISTED_OUTPUT}={listed}"));
    }
    if let Some(packages) = counts.by_package
        && let Some(encoded) = encode_package_counts(&packages)
    {
        lines.push(format!("{TESTS_BY_PACKAGE_OUTPUT}={encoded}"));
    }
    lines.join("\n") + "\n"
}

#[must_use]
pub(crate) fn list_argv(run: &[String]) -> Option<Vec<String>> {
    if !runs_nextest(run) {
        return None;
    }
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
pub(crate) fn runnable_matches(bytes: &[u8]) -> Option<BTreeMap<String, u64>> {
    let listed: Listed = serde_json::from_slice(bytes).ok()?;
    let mut packages = BTreeMap::new();
    let mut excluded = BTreeMap::new();
    for suite in listed.rust_suites.into_values() {
        if !package_token(&suite.package_name) {
            return None;
        }
        let mut matched = 0u64;
        let mut mismatched = 0u64;
        for case in suite.testcases.into_values() {
            match case.filter_match.status.as_str() {
                "matches" => matched = matched.checked_add(1)?,
                "mismatch" => mismatched = mismatched.checked_add(1)?,
                _ => return None,
            }
        }
        add(&mut packages, &suite.package_name, matched)?;
        add(&mut excluded, &suite.package_name, mismatched)?;
    }
    Some(if packages.is_empty() {
        excluded
    } else {
        packages
    })
}

fn add(into: &mut BTreeMap<String, u64>, name: &str, count: u64) -> Option<()> {
    if count > 0 {
        let total = into.get(name).copied().unwrap_or(0_u64);
        into.insert(name.to_owned(), total.checked_add(count)?);
    }
    Some(())
}

#[must_use]
pub(crate) fn apply_runnable(
    mut counts: TestCounts,
    matches: Option<BTreeMap<String, u64>>,
) -> TestCounts {
    let Some(matches) = matches else {
        counts.listed = None;
        counts.by_package = None;
        return counts;
    };
    let Some(total) = matches
        .values()
        .try_fold(0u64, |total, count| total.checked_add(*count))
    else {
        counts.listed = None;
        counts.by_package = None;
        return counts;
    };
    counts.listed = Some(total);
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

#[derive(Debug)]
enum Seen {
    Open(BTreeMap<String, u64>),
    Poison,
}

impl Default for Seen {
    fn default() -> Self {
        Self::Open(BTreeMap::new())
    }
}

#[derive(Debug, Default)]
pub struct SummaryTap {
    line: Vec<u8>,
    overlong: bool,
    counts: Option<TestCounts>,
    seen: Seen,
    closed: bool,
}

impl SummaryTap {
    pub fn feed(&mut self, chunk: &[u8]) {
        for part in chunk.split_inclusive(|byte| *byte == b'\n') {
            let (text, ended) = match part.strip_suffix(b"\n") {
                Some(text) => (text, true),
                None => (part, false),
            };
            let room = LINE_LIMIT.saturating_sub(self.line.len());
            if text.len() > room {
                self.overlong = true;
            }
            self.line.extend_from_slice(&text[..text.len().min(room)]);
            if ended {
                self.end_line();
            }
        }
    }

    #[must_use]
    pub fn counts(&self) -> Option<TestCounts> {
        self.counts.clone()
    }

    #[must_use]
    pub fn finish(mut self) -> Option<TestCounts> {
        self.end_line();
        self.counts
    }

    fn end_line(&mut self) {
        let line = String::from_utf8_lossy(&self.line);
        let cleaned = strip_ansi(&line);
        let trimmed = cleaned.trim_start();
        let summary = if self.overlong {
            None
        } else {
            nextest_summary(trimmed)
        };
        if let Some(summary) = summary {
            let runs = self.reconcile_runs(summary.tests_run);
            self.closed = true;
            self.counts = Some(TestCounts {
                run: summary.tests_run,
                listed: None,
                by_package: None,
                runs,
            });
        } else if !self.closed {
            let trimmed = trimmed.to_owned();
            self.note(&trimmed);
        }
        self.line.clear();
        self.overlong = false;
    }

    fn reconcile_runs(&self, run: u64) -> Option<BTreeMap<String, u64>> {
        let Seen::Open(packages) = &self.seen else {
            return None;
        };
        let summed = packages
            .values()
            .try_fold(0u64, |total, count| total.checked_add(*count))?;
        (summed == run).then(|| packages.clone())
    }

    fn note(&mut self, line: &str) {
        match status_line(line) {
            Status::Ignore => {}
            Status::Poison => self.seen = Seen::Poison,
            Status::Count { package } => self.count(package),
        }
    }

    fn count(&mut self, package: String) {
        let overflow = {
            let Seen::Open(packages) = &mut self.seen else {
                return;
            };
            let entry = packages.entry(package).or_default();
            match entry.checked_add(1) {
                Some(next) => {
                    *entry = next;
                    false
                }
                None => true,
            }
        };
        if overflow {
            self.seen = Seen::Poison;
        }
    }
}

enum Status {
    Ignore,
    Poison,
    Count { package: String },
}

fn status_line(line: &str) -> Status {
    let mut words = line.split_whitespace();
    let Some(first) = words.next() else {
        return Status::Ignore;
    };
    let (word, retried) = if first == "TRY" {
        let numbered = words
            .next()
            .is_some_and(|attempt| attempt.bytes().all(|byte| byte.is_ascii_digit()));
        match words.next() {
            Some(word) if numbered => (word, true),
            _ => return Status::Ignore,
        }
    } else {
        (first, false)
    };
    if !(matches!(word, "PASS" | "LEAK") || (word == "FAIL" && !retried)) {
        return Status::Ignore;
    }
    let Some((_, rest)) = line.split_once(word) else {
        return Status::Ignore;
    };
    let rest = rest.trim_start();
    if !rest.starts_with('[') {
        return Status::Ignore;
    }
    parse_status(rest).map_or(Status::Poison, |package| Status::Count { package })
}

fn parse_status(rest: &str) -> Option<String> {
    let rest = rest.strip_prefix('[')?;
    let (inside, rest) = rest.split_once(']')?;
    if !duration_ok(inside) {
        return None;
    }
    let rest = rest.trim_start();
    let rest = match rest.strip_prefix('(') {
        Some(counter) => counter.split_once(')')?.1,
        None => rest,
    };
    let binary = rest.split_whitespace().next()?;
    let package = binary.split("::").next()?;
    package_token(package).then(|| package.to_owned())
}

fn duration_ok(inside: &str) -> bool {
    let Some(number) = inside.trim().strip_suffix('s') else {
        return false;
    };
    let mut parts = number.trim().split('.');
    let whole = parts.next().unwrap_or("");
    let fraction = parts.next();
    parts.next().is_none()
        && !whole.is_empty()
        && whole.bytes().all(|byte| byte.is_ascii_digit())
        && fraction.is_none_or(|fraction| {
            !fraction.is_empty() && fraction.bytes().all(|byte| byte.is_ascii_digit())
        })
}

#[derive(Deserialize)]
struct Listed {
    #[serde(rename = "rust-suites")]
    rust_suites: BTreeMap<String, Suite>,
}

#[derive(Deserialize)]
struct Suite {
    #[serde(rename = "package-name")]
    package_name: String,
    testcases: BTreeMap<String, Case>,
}

#[derive(Deserialize)]
struct Case {
    #[serde(rename = "filter-match")]
    filter_match: FilterMatch,
}

#[derive(Deserialize)]
struct FilterMatch {
    status: String,
}
