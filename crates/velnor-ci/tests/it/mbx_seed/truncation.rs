// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::Path;
use std::time::Duration;

use velnor_ci::mbx_seed::record_truncation;
use velnor_ci::waste_evidence::{SEED_TRUNCATED, seed_truncated};

use super::{host, read_only, run, run_within, warm_seed};

#[test]
fn a_truncated_seed_is_recorded_for_the_report_and_a_complete_one_is_not() {
    let host = host("mbx-seed-truncation-record");
    let seed = warm_seed(&host);
    let temp = host.root.join("temp");
    let cut = run_within(
        &host,
        "pull_request",
        "refs/pull/1/merge",
        Some(&seed),
        Duration::ZERO,
    );
    assert!(!seed_truncated(&temp));
    assert!(record_truncation(&temp, &cut).is_ok());
    assert!(seed_truncated(&temp));
    fs::remove_file(temp.join(SEED_TRUNCATED)).unwrap_or_else(|e| unreachable!("{e}"));
    let whole = run(&host, "pull_request", "refs/pull/1/merge", Some(&seed));
    assert!(record_truncation(&temp, &whole).is_ok());
    assert!(!seed_truncated(&temp));
}

#[test]
fn a_run_that_seeds_nothing_keeps_an_earlier_truncation_marker() {
    let host = host("mbx-seed-truncation-keep");
    let seed = warm_seed(&host);
    let temp = host.root.join("temp");
    let cut = run_within(
        &host,
        "pull_request",
        "refs/pull/1/merge",
        Some(&seed),
        Duration::ZERO,
    );
    assert!(record_truncation(&temp, &cut).is_ok());
    assert!(seed_truncated(&temp));
    let empty = host.root.join("empty");
    fs::create_dir_all(&empty).unwrap_or_else(|e| unreachable!("{e}"));
    read_only(&empty);
    let others = [
        run(&host, "pull_request", "refs/pull/1/merge", None),
        run(&host, "push", "refs/heads/main", Some(&seed)),
        run(
            &host,
            "pull_request",
            "refs/pull/1/merge",
            Some(Path::new("relative")),
        ),
        run(&host, "pull_request", "refs/pull/1/merge", Some(&empty)),
    ];
    for outcome in others {
        assert!(record_truncation(&temp, &outcome).is_ok());
        assert!(seed_truncated(&temp), "{outcome:?}");
    }
}

#[test]
fn a_complete_seed_clears_a_marker_left_by_an_earlier_run() {
    let host = host("mbx-seed-truncation-stale");
    let seed = warm_seed(&host);
    let temp = host.root.join("temp");
    let cut = run_within(
        &host,
        "pull_request",
        "refs/pull/1/merge",
        Some(&seed),
        Duration::ZERO,
    );
    assert!(record_truncation(&temp, &cut).is_ok());
    assert!(seed_truncated(&temp));
    let whole = run(&host, "pull_request", "refs/pull/1/merge", Some(&seed));
    assert!(record_truncation(&temp, &whole).is_ok());
    assert!(!seed_truncated(&temp));
}
