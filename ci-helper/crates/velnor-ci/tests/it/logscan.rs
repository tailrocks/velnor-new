// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_ci::logscan::{CacheKind, scan};

use crate::support::{CLI_LOG, CORE_LOG};

#[test]
fn recorded_test_job_log_is_counted() {
    let result = scan(CLI_LOG);
    assert_eq!(result.compiling, 24);
    assert_eq!(result.checking, 2);
    assert_eq!(result.documenting, 0);
    assert_eq!(result.downloaded, 0);
    assert_eq!(result.finished_seconds, vec![49.84, 26.87, 4.92]);
    assert_eq!(result.nextest.len(), 1);
    assert_eq!(result.nextest[0].tests_run, 221);
    assert!((result.nextest[0].seconds - 597.553).abs() < 1e-9);
    assert_eq!(result.caches.len(), 2);
    assert_eq!(result.caches[0].kind, CacheKind::Restored);
    assert_eq!(result.caches[0].size_bytes, Some(400_627_507));
    assert_eq!(
        result.caches[0].key.as_deref(),
        Some("mise-tools-v2-351e03e56a119396b95fbfa7963a97987cee3a1ba924b87a835ded25c0b4699d")
    );
    assert_eq!(result.caches[1].size_bytes, Some(1_457_119_465));
}

#[test]
fn recorded_check_job_log_counts_checking_and_documenting() {
    let result = scan(CORE_LOG);
    assert_eq!(result.compiling, 0);
    assert_eq!(result.checking, 1);
    assert_eq!(result.documenting, 1);
    assert_eq!(result.crates_compiled(), 2);
    assert_eq!(result.finished_seconds, vec![0.99, 0.90]);
    assert_eq!(result.nextest.len(), 0);
    assert_eq!(result.caches.len(), 1);
    assert_eq!(result.caches[0].size_bytes, Some(400_627_507));
    assert_eq!(result.caches[0].key, None);
}

#[test]
fn lines_that_only_look_similar_are_not_counted() {
    let log = "\
2026-11-12T08:38:34.2205888Z ##[group]Checking out the ref
2026-11-12T08:38:34.2205888Z Finished waiting for all background step(s).
2026-11-12T08:38:34.2205888Z Finished uploading artifact content to blob storage!
2026-11-12T08:38:34.2205888Z     Compiling workspace without a version
2026-11-12T08:38:34.2205888Z   Downloaded 12 crates (3.1MB) in 1.2s
2026-11-12T08:38:34.2205888Z Starting download of artifact to: /tmp/x
2026-11-12T08:38:34.2205888Z Cache hit for: some-key
";
    let result = scan(log);
    assert_eq!(result.crates_compiled(), 0);
    assert_eq!(result.downloaded, 0);
    assert_eq!(result.finished_seconds.len(), 0);
    assert_eq!(result.caches.len(), 0);
}

#[test]
fn ansi_colours_and_timestamps_are_removed_before_matching() {
    let log = "\
2026-11-12T08:38:34.2205888Z \u{1b}[1m\u{1b}[32m   Compiling\u{1b}[0m serde v1.0.229
2026-11-12T08:38:34.2205888Z \u{1b}[1m\u{1b}[32m  Downloaded\u{1b}[0m serde v1.0.229
2026-11-12T08:38:34.2205888Z \u{1b}[1m\u{1b}[32m    Finished\u{1b}[0m `release` profile [optimized] target(s) in 1m 02s
2026-11-12T08:38:34.2205888Z      Summary [   0.028s] 5 tests run: 5 passed, 0 skipped
";
    let result = scan(log);
    assert_eq!(result.compiling, 1);
    assert_eq!(result.downloaded, 1);
    assert_eq!(result.finished_seconds, vec![62.0]);
    assert_eq!(result.nextest[0].tests_run, 5);
}

#[test]
fn saved_cache_reports_size_and_key() {
    let log = "\
2026-11-12T08:38:34.2205888Z Cache Size: ~79 MB (82437083 B)
2026-11-12T08:38:34.2205888Z Cache saved successfully
2026-11-12T08:38:34.2205888Z Cache saved with key: sources-v1-abc
";
    let result = scan(log);
    assert_eq!(result.caches.len(), 1);
    assert_eq!(result.caches[0].kind, CacheKind::Saved);
    assert_eq!(result.caches[0].size_bytes, Some(82_437_083));
    assert_eq!(result.caches[0].key.as_deref(), Some("sources-v1-abc"));
}
