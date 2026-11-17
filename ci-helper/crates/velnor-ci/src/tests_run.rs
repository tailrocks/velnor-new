// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::TESTS_RUN_OUTPUT;

use crate::logscan::{nextest_summary, strip_ansi};

pub const TESTS_LISTED_OUTPUT: &str = "tests_listed";

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TestCounts {
    pub run: u64,
    pub listed: Option<u64>,
}

#[must_use]
pub fn output_lines(counts: TestCounts) -> String {
    let run = format!("{TESTS_RUN_OUTPUT}={}\n", counts.run);
    match counts.listed {
        Some(listed) => format!("{run}{TESTS_LISTED_OUTPUT}={listed}\n"),
        None => run,
    }
}

fn skipped(line: &str) -> Option<u64> {
    let (_, tail) = line.split_once(" run:")?;
    tail.split(',')
        .find_map(|part| part.trim().strip_suffix(" skipped"))
        .and_then(|count| count.trim().parse().ok())
}

#[derive(Debug, Default)]
pub struct SummaryTap {
    line: Vec<u8>,
    overlong: bool,
    counts: Option<TestCounts>,
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
    pub const fn counts(&self) -> Option<TestCounts> {
        self.counts
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
            if let Some(summary) = nextest_summary(cleaned.trim_start()) {
                self.counts = Some(TestCounts {
                    run: summary.tests_run,
                    listed: skipped(cleaned.trim_start())
                        .and_then(|skipped| skipped.checked_add(summary.tests_run)),
                });
            }
        }
        self.line.clear();
        self.overlong = false;
    }
}
