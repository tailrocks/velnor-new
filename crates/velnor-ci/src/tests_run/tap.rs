// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use crate::logscan::{nextest_summary, strip_ansi};

use super::{LINE_LIMIT, TestCounts, skipped, status};

#[derive(Debug)]
enum Seen {
    Open {
        with_counter: BTreeMap<String, u64>,
        without_counter: BTreeMap<String, u64>,
    },
    Poison,
}

impl Default for Seen {
    fn default() -> Self {
        Self::Open {
            with_counter: BTreeMap::new(),
            without_counter: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Default)]
pub struct SummaryTap {
    line: Vec<u8>,
    overlong: bool,
    counts: Option<TestCounts>,
    seen: Seen,
    closed: bool,
    started: bool,
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
    pub fn started(&self) -> bool {
        self.started
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
        if !self.overlong && trimmed.starts_with("Starting ") && trimmed.contains(" across ") {
            self.started = true;
        }
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
                listed: skipped(trimmed).and_then(|skipped| skipped.checked_add(summary.tests_run)),
                ignored: None,
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
        let Seen::Open {
            with_counter,
            without_counter,
        } = &self.seen
        else {
            return None;
        };
        if summed(with_counter) == Some(run) {
            return Some(with_counter.clone());
        }
        if summed(without_counter) == Some(run) {
            return Some(without_counter.clone());
        }
        None
    }

    fn note(&mut self, line: &str) {
        match status::status_line(line) {
            status::Status::Ignore => {}
            status::Status::Poison => self.seen = Seen::Poison,
            status::Status::Count { package, counter } => self.count(package, counter),
        }
    }

    fn count(&mut self, package: String, counter: bool) {
        let overflow = {
            let Seen::Open {
                with_counter,
                without_counter,
            } = &mut self.seen
            else {
                return;
            };
            let packages = if counter {
                with_counter
            } else {
                without_counter
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

fn summed(packages: &BTreeMap<String, u64>) -> Option<u64> {
    packages
        .values()
        .try_fold(0u64, |total, count| total.checked_add(*count))
}
