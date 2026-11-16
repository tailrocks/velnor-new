// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_model::{TESTS_BY_PACKAGE_OUTPUT, TESTS_LISTED_OUTPUT, TESTS_RUN_OUTPUT};
use velnor_rust::{PackageCounts, encode_package_counts};

use crate::logscan::{nextest_summary, strip_ansi};

const LINE_LIMIT: usize = 4096;

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

fn skipped(line: &str) -> Option<u64> {
    let (_, tail) = line.split_once(" run:")?;
    tail.split(',')
        .find_map(|part| part.trim().strip_suffix(" skipped"))
        .and_then(|count| count.trim().parse().ok())
}

#[derive(Debug)]
enum Seen {
    Open(BTreeMap<String, PackageCounts>),
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
            if self.line.len() + text.len() > LINE_LIMIT {
                self.overlong = true;
                self.line.clear();
            } else if !self.overlong {
                self.line.extend_from_slice(text);
            }
            if ended {
                self.end_line();
            }
        }
    }

    #[must_use]
    pub fn finish(mut self) -> Option<TestCounts> {
        self.end_line();
        self.counts
    }

    fn end_line(&mut self) {
        if !self.overlong {
            let line = String::from_utf8_lossy(&self.line);
            let cleaned = strip_ansi(&line);
            let trimmed = cleaned.trim_start();
            if let Some(summary) = nextest_summary(trimmed) {
                let listed =
                    skipped(trimmed).and_then(|skipped| skipped.checked_add(summary.tests_run));
                let by_package =
                    listed.and_then(|listed| self.reconcile(summary.tests_run, listed));
                self.closed = true;
                self.counts = Some(TestCounts {
                    run: summary.tests_run,
                    listed,
                    by_package,
                });
            } else if !self.closed {
                self.note(trimmed);
            }
        }
        self.line.clear();
        self.overlong = false;
    }

    fn reconcile(&self, run: u64, listed: u64) -> Option<BTreeMap<String, PackageCounts>> {
        let Seen::Open(packages) = &self.seen else {
            return None;
        };
        let summed_run = packages
            .values()
            .try_fold(0u64, |total, package| total.checked_add(package.run))?;
        let summed_listed = packages
            .values()
            .try_fold(0u64, |total, package| total.checked_add(package.listed))?;
        (summed_run == run && summed_listed == listed).then(|| packages.clone())
    }

    fn note(&mut self, line: &str) {
        match status_line(line) {
            Status::Ignore => {}
            Status::Poison => self.seen = Seen::Poison,
            Status::Count { package, ran } => self.count(package, ran),
        }
    }

    fn count(&mut self, package: String, ran: bool) {
        let overflow = {
            let Seen::Open(packages) = &mut self.seen else {
                return;
            };
            let entry = packages.entry(package).or_default();
            let run = if ran {
                entry.run.checked_add(1)
            } else {
                Some(entry.run)
            };
            match (run, entry.listed.checked_add(1)) {
                (Some(run), Some(listed)) => {
                    entry.run = run;
                    entry.listed = listed;
                    false
                }
                _ => true,
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
    Count { package: String, ran: bool },
}

fn status_line(line: &str) -> Status {
    let Some(word) = line.split_whitespace().next() else {
        return Status::Ignore;
    };
    let ran = match word {
        "PASS" | "FAIL" | "LEAK" => true,
        "SKIP" => false,
        _ => return Status::Ignore,
    };
    let Some(rest) = line.strip_prefix(word).map(str::trim_start) else {
        return Status::Ignore;
    };
    if !rest.starts_with('[') {
        return Status::Ignore;
    }
    parse_status(rest, ran).map_or(Status::Poison, |(package, ran)| Status::Count {
        package,
        ran,
    })
}

fn parse_status(rest: &str, ran: bool) -> Option<(String, bool)> {
    let rest = rest.strip_prefix('[')?;
    let (inside, rest) = rest.split_once(']')?;
    if !bracket_ok(inside, ran) {
        return None;
    }
    let rest = rest.trim_start().strip_prefix('(')?;
    let (_, rest) = rest.split_once(')')?;
    let binary = rest.split_whitespace().next()?;
    let package = binary.split("::").next()?;
    package_token(package).then(|| (package.to_owned(), ran))
}

fn bracket_ok(inside: &str, ran: bool) -> bool {
    if !ran {
        return inside.bytes().all(|byte| byte == b' ');
    }
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

fn package_token(name: &str) -> bool {
    let mut bytes = name.bytes();
    match bytes.next() {
        Some(byte) if byte.is_ascii_alphanumeric() || byte == b'_' => {}
        _ => return false,
    }
    bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}
