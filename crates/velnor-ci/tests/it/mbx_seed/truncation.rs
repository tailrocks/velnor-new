// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::time::Duration;

use velnor_ci::mbx_seed::record_truncation;
use velnor_ci::waste_evidence::{SEED_TRUNCATED, seed_truncated};

use super::{host, run, run_within, warm_seed};

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
