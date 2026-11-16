// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::TESTS_RUN_OUTPUT;

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

#[must_use]
pub fn output_line(tests_run: u64) -> String {
    format!("{TESTS_RUN_OUTPUT}={tests_run}\n")
}

#[derive(Debug, Default)]
pub struct SummaryTap {
    line: Vec<u8>,
    overlong: bool,
    tests_run: Option<u64>,
}

impl SummaryTap {
    #[must_use]
    pub const fn tests_run(&self) -> Option<u64> {
        self.tests_run
    }

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
    pub fn finish(mut self) -> Option<u64> {
        self.end_line();
        self.tests_run
    }

    fn end_line(&mut self) {
        if !self.overlong {
            let line = String::from_utf8_lossy(&self.line);
            let cleaned = strip_ansi(&line);
            if let Some(summary) = nextest_summary(cleaned.trim_start()) {
                self.tests_run = Some(summary.tests_run);
            }
        }
        self.line.clear();
        self.overlong = false;
    }
}
