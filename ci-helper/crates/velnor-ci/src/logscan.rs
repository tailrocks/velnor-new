// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LogScan {
    pub compiling: u64,
    pub checking: u64,
    pub documenting: u64,
    pub downloaded: u64,
    pub finished_seconds: Vec<f64>,
    pub nextest: Vec<NextestSummary>,
    pub caches: Vec<CacheEvent>,
}

impl LogScan {
    #[must_use]
    pub fn crates_compiled(&self) -> u64 {
        self.compiling + self.checking + self.documenting
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NextestSummary {
    pub tests_run: u64,
    pub seconds: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CacheKind {
    Restored,
    Saved,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CacheEvent {
    pub kind: CacheKind,
    pub key: Option<String>,
    pub size_bytes: Option<u64>,
}

#[must_use]
pub fn scan(log: &str) -> LogScan {
    let mut result = LogScan::default();
    let mut pending_size = None;
    for raw in log.lines() {
        let cleaned = strip_ansi(strip_timestamp(raw));
        let line = cleaned.trim_start();
        scan_line(line, &mut result, &mut pending_size);
    }
    result
}

#[must_use]
pub fn strip_timestamp(line: &str) -> &str {
    match line.find(' ') {
        Some(end) if end > 0 && line[..end].ends_with('Z') => &line[end + 1..],
        _ => line,
    }
}

#[must_use]
pub fn strip_ansi(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next();
            for next in chars.by_ref() {
                if ('@'..='~').contains(&next) {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn scan_line(line: &str, result: &mut LogScan, pending_size: &mut Option<u64>) {
    if has_versioned_crate(line, "Compiling") {
        result.compiling += 1;
    } else if has_versioned_crate(line, "Checking") {
        result.checking += 1;
    } else if has_versioned_crate(line, "Documenting") {
        result.documenting += 1;
    } else if has_versioned_crate(line, "Downloaded") {
        result.downloaded += 1;
    } else if let Some(seconds) = finished_seconds(line) {
        result.finished_seconds.push(seconds);
    } else if let Some(summary) = nextest_summary(line) {
        result.nextest.push(summary);
    } else if line.starts_with("Cache ") {
        scan_cache_line(line, result, pending_size);
    }
}

fn has_versioned_crate(line: &str, word: &str) -> bool {
    let Some(rest) = line
        .strip_prefix(word)
        .and_then(|rest| rest.strip_prefix(' '))
    else {
        return false;
    };
    match rest.find(char::is_whitespace) {
        Some(end) if end > 0 => rest[end..].starts_with(" v"),
        _ => false,
    }
}

fn finished_seconds(line: &str) -> Option<f64> {
    let rest = line.strip_prefix("Finished ")?;
    let (_, duration) = rest.rsplit_once(" in ")?;
    parse_duration(duration)
}

fn parse_duration(text: &str) -> Option<f64> {
    let mut total = 0.0;
    let mut tokens = 0;
    for token in text.split_whitespace() {
        let (number, factor) = if let Some(number) = token.strip_suffix('h') {
            (number, 3600.0)
        } else if let Some(number) = token.strip_suffix('m') {
            (number, 60.0)
        } else {
            (token.strip_suffix('s')?, 1.0)
        };
        total += number.parse::<f64>().ok()? * factor;
        tokens += 1;
    }
    (tokens > 0).then_some(total)
}

fn nextest_summary(line: &str) -> Option<NextestSummary> {
    let rest = line.strip_prefix("Summary [")?;
    let (seconds, rest) = rest.split_once("s]")?;
    let seconds = seconds.trim().parse().ok()?;
    let rest = rest.trim_start();
    let (count, _) = rest
        .split_once(" tests run")
        .or_else(|| rest.split_once(" test run"))?;
    Some(NextestSummary {
        tests_run: count.parse().ok()?,
        seconds,
    })
}

fn scan_cache_line(line: &str, result: &mut LogScan, pending_size: &mut Option<u64>) {
    if let Some(rest) = line.strip_prefix("Cache Size:") {
        *pending_size = rest
            .rsplit_once('(')
            .and_then(|(_, tail)| tail.strip_suffix(" B)"))
            .and_then(|bytes| bytes.parse().ok());
    } else if line.starts_with("Cache restored successfully") {
        push_cache(result, CacheKind::Restored, pending_size);
    } else if line.starts_with("Cache saved successfully") {
        push_cache(result, CacheKind::Saved, pending_size);
    } else if let Some(key) = line.strip_prefix("Cache restored from key:") {
        set_cache_key(result, CacheKind::Restored, key.trim());
    } else if let Some(key) = line.strip_prefix("Cache saved with key:") {
        set_cache_key(result, CacheKind::Saved, key.trim());
    }
}

fn push_cache(result: &mut LogScan, kind: CacheKind, pending_size: &mut Option<u64>) {
    result.caches.push(CacheEvent {
        kind,
        key: None,
        size_bytes: pending_size.take(),
    });
}

fn set_cache_key(result: &mut LogScan, kind: CacheKind, key: &str) {
    match result.caches.last_mut() {
        Some(last) if last.kind == kind && last.key.is_none() => last.key = Some(key.to_owned()),
        _ => result.caches.push(CacheEvent {
            kind,
            key: Some(key.to_owned()),
            size_bytes: None,
        }),
    }
}
