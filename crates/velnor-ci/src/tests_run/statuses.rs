// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use super::{Listing, PackageCounts, SummaryTap, apply_runnable};

#[test]
fn terminal_statuses_reconcile_with_nextest_summary() {
    let mut tap = SummaryTap::default();
    tap.feed(
        b"PASS [   0.012s] (1/10) kept pass\n\
          FAIL [   0.012s] (2/10) kept fail\n\
          TIMEOUT [   0.012s] (3/10) kept timeout\n\
          TIMEOUT-PASS [   0.012s] (4/10) kept timeout_pass\n\
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
        apply_runnable(
            counts,
            Some(Listing {
                packages: listed,
                ignored: 0
            })
        )
        .by_package,
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

    assert_eq!(
        apply_runnable(
            counts,
            Some(Listing {
                packages: listed,
                ignored: 0
            })
        )
        .by_package,
        None
    );
}

#[test]
fn real_nextest_output_reconciles_with_its_summary() {
    let mut tap = SummaryTap::default();
    tap.feed(
        b"    Starting 6 tests across 1 binary (1 test skipped)\n\
          \x20    SIGABRT [   0.006s] (1/6) kept t::aborts\n\
          \x20       FAIL [   0.006s] (2/6) kept t::bad\n\
          \x20       PASS [   0.007s] (3/6) kept t::ok\n\
          \x20  LEAK-FAIL [   0.209s] (4/6) kept t::leaky\n\
          TIMEOUT-PASS [   1.002s] (5/6) kept t::slow_pass\n\
          \x20    TIMEOUT [   1.002s] (6/6) kept t::slow_fail\n\
          \x20    Summary [   1.008s] 6 tests run: 2 passed, 3 failed (1 due to being leaky), 1 timed out, 1 skipped\n\
          \x20    SIGABRT [   0.006s] (1/6) kept t::aborts\n\
          \x20       FAIL [   0.006s] (2/6) kept t::bad\n\
          \x20  LEAK-FAIL [   0.209s] (4/6) kept t::leaky\n\
          \x20    TIMEOUT [   1.002s] (6/6) kept t::slow_fail\n",
    );
    let counts = tap.finish().expect("nextest summary");
    let listed = BTreeMap::from([("kept".to_owned(), 6)]);

    assert_eq!(
        apply_runnable(
            counts,
            Some(Listing {
                packages: listed,
                ignored: 0
            })
        )
        .by_package,
        Some(BTreeMap::from([(
            "kept".to_owned(),
            PackageCounts { run: 6, listed: 6 },
        )]))
    );
}

#[test]
fn a_nextest_setup_script_line_does_not_poison_package_counts() {
    let mut tap = SummaryTap::default();
    tap.feed(
        b"    Starting 2 tests across 1 binary (1 test skipped)\n\
          \x20     SETUP [      1/1] prepull-images: bash .config/nextest-prepull-images.sh\n\
          nextest-prepull-images: pulling postgres:18-alpine\n\
          \x20SETUP PASS [   9.804s] prepull-images: bash .config/nextest-prepull-images.sh\n\
          \x20       PASS [   0.012s] (1/2) kept alpha\n\
          \x20       PASS [   0.012s] (2/2) kept beta\n\
          \x20    Summary [   0.022s] 2 tests run: 2 passed, 1 skipped\n",
    );
    let counts = tap.finish().expect("nextest summary");
    let listed = BTreeMap::from([("kept".to_owned(), 2)]);

    assert_eq!(
        apply_runnable(
            counts,
            Some(Listing {
                packages: listed,
                ignored: 0
            })
        )
        .by_package,
        Some(BTreeMap::from([(
            "kept".to_owned(),
            PackageCounts { run: 2, listed: 2 },
        )]))
    );
}
