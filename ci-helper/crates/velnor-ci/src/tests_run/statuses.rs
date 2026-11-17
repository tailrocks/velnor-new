// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use super::{PackageCounts, SummaryTap, apply_runnable};

#[test]
fn terminal_statuses_reconcile_with_nextest_summary() {
    let mut tap = SummaryTap::default();
    tap.feed(
        b"PASS [   0.012s] (1/10) kept pass\n\
          FAIL [   0.012s] (2/10) kept fail\n\
          TIMEOUT [   0.012s] (3/10) kept timeout\n\
          TMPASS [   0.012s] (4/10) kept timeout_pass\n\
          LEAK [   0.012s] (5/10) kept leak\n\
          LEAK-FAIL [   0.012s] (6/10) kept leak_fail\n\
          SIGSEGV [   0.012s] (7/10) kept signal\n\
          ABORT [   0.012s] (8/10) kept abort\n\
          TRY 1 FAIL [   0.012s] (9/10) kept retry\n\
          TRY 2 PASS [   0.012s] (9/10) kept retry\n\
          SKIP [         ] (10/10) kept skip\n\
          Summary [   0.022s] 9 tests run: 4 passed, 5 failed, 1 skipped\n\
          FLAKY 9/10 [   0.016s] (9/10) kept retry\n",
    );
    let counts = tap.finish().expect("nextest summary");
    let listed = BTreeMap::from([("kept".to_owned(), 10)]);

    assert_eq!(
        apply_runnable(counts, Some(listed)).by_package,
        Some(BTreeMap::from([(
            "kept".to_owned(),
            PackageCounts { run: 9, listed: 10 },
        )]))
    );
}

#[test]
fn unknown_status_poisons_package_counts() {
    let mut tap = SummaryTap::default();
    tap.feed(
        b"MYSTERY [   0.012s] (1/1) kept test\n\
          PASS [   0.012s] (1/1) kept test\n\
          Summary [   0.022s] 1 test run: 1 passed, 0 skipped\n",
    );
    let counts = tap.finish().expect("nextest summary");
    let listed = BTreeMap::from([("kept".to_owned(), 1)]);

    assert_eq!(apply_runnable(counts, Some(listed)).by_package, None);
}
