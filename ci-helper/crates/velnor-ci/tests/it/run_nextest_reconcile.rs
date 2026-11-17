// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#![cfg(unix)]

use std::collections::BTreeMap;

use velnor_model::{JobId, VerdictSpec};
use velnor_rust::{Need, decide};

use crate::run_nextest::{FILTERED_LIST, NEXTEST_RUN, listed, run_with_output};

#[test]
fn run_counts_packages_by_the_tests_the_filter_matches() {
    let script = r"cat >&2 <<'END'
    SKIP [         ] (───) kept beta
    SKIP [         ] (───) only-ign hidden
    PASS [   0.012s] (1/2) kept alpha
    PASS [   0.012s] (2/2) both unit
 Summary [   0.022s] 2 tests run: 2 passed, 2 skipped
END";
    let (output, written, _) =
        run_with_output("tests-run-packages", script, FILTERED_LIST, &NEXTEST_RUN);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        written,
        "existing=1\ntests_run=2\ntests_listed=3\ntests_by_package=both=1/1 kept=1/2\n"
    );
}

#[test]
fn run_counts_a_failure_once_when_nextest_prints_it_again_after_the_summary() {
    let script = r"cat >&2 <<'END'
    FAIL [   0.007s] (1/1) ran tests::boom
 Summary [   0.007s] 1 test run: 0 passed, 1 failed, 0 skipped
    FAIL [   0.007s] (1/1) ran tests::boom
 Summary [   0.007s] 1 test run: 0 passed, 1 failed, 0 skipped
END";
    let one = listed(&[("ran", &["boom"])]);
    let (output, written, _) = run_with_output("tests-run-fail-once", script, &one, &NEXTEST_RUN);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(
        written,
        "existing=1\ntests_run=1\ntests_listed=1\ntests_by_package=ran=1/1\n"
    );
}

#[test]
fn run_reports_a_package_that_had_runnable_tests_and_ran_none() {
    let script = r"cat >&2 <<'END'
    PASS [   0.012s] (1/1) kept alpha
 Summary [   0.012s] 1 test run: 1 passed, 0 skipped
END";
    let counts = listed(&[("idle", &["one", "two"]), ("kept", &["alpha"])]);
    let (_, written, _) = run_with_output("tests-run-idle", script, &counts, &NEXTEST_RUN);
    assert_eq!(
        written,
        "existing=1\ntests_run=1\ntests_listed=3\ntests_by_package=idle=0/2 kept=1/1\n"
    );
}

#[test]
fn run_omits_the_package_map_when_a_package_ran_more_than_it_matched() {
    let script = r"cat >&2 <<'END'
    PASS [   0.012s] (1/2) kept alpha
    PASS [   0.012s] (2/2) kept beta
 Summary [   0.012s] 2 tests run: 2 passed, 0 skipped
END";
    let one = listed(&[("kept", &["alpha"])]);
    let (_, written, _) = run_with_output("tests-run-exceed", script, &one, &NEXTEST_RUN);
    assert_eq!(written, "existing=1\ntests_run=2\ntests_listed=1\n");
}

#[test]
fn run_omits_the_package_map_when_the_lines_do_not_reconcile() {
    let short = r"cat >&2 <<'END'
    PASS [   0.010s] (1/2) ran tests::alpha
 Summary [   0.010s] 2 tests run: 2 passed, 0 skipped
END";
    let two = listed(&[("ran", &["alpha", "beta"])]);
    let (_, written, _) = run_with_output("tests-run-short", short, &two, &NEXTEST_RUN);
    assert_eq!(written, "existing=1\ntests_run=2\ntests_listed=2\n");
    let hostile = r"cat >&2 <<'END'
    PASS [nope] (1/1) ran tests::t
 Summary [   0.010s] 1 test run: 1 passed, 0 skipped
END";
    let one = listed(&[("ran", &["t"])]);
    let (_, written, _) = run_with_output("tests-run-hostile", hostile, &one, &NEXTEST_RUN);
    assert_eq!(written, "existing=1\ntests_run=1\ntests_listed=1\n");
    let named = r"cat >&2 <<'END'
    PASS [   0.010s] (1/1) bad=name tests::t
 Summary [   0.010s] 1 test run: 1 passed, 0 skipped
END";
    let (_, written, _) = run_with_output("tests-run-bad-name", named, &one, &NEXTEST_RUN);
    assert_eq!(written, "existing=1\ntests_run=1\ntests_listed=1\n");
    let prose = r"cat >&2 <<'END'
PASS the exam
    PASS [   0.010s] (1/1) ran tests::alpha
 Summary [   0.010s] 1 test run: 1 passed, 0 skipped
END";
    let ran = listed(&[("ran", &["alpha"])]);
    let (_, written, _) = run_with_output("tests-run-prose", prose, &ran, &NEXTEST_RUN);
    assert_eq!(
        written,
        "existing=1\ntests_run=1\ntests_listed=1\ntests_by_package=ran=1/1\n"
    );
}

#[test]
fn run_keeps_a_package_whose_tests_the_filter_excluded_next_to_one_that_ran() {
    let script = r"cat >&2 <<'END'
    PASS [   0.012s] (1/1) kept alpha
 Summary [   0.012s] 1 test run: 1 passed, 0 skipped
END";
    let list = concat!(
        r##"{"test-count":3,"rust-suites":{"##,
        r##""kept":{"package-name":"kept","status":"listed","testcases":{"alpha":{"ignored":false,"filter-match":{"status":"matches"}}}},"##,
        r##""gone":{"package-name":"gone","status":"listed","testcases":{"one":{"ignored":false,"filter-match":{"status":"mismatch","reason":"expression"}},"two":{"ignored":false,"filter-match":{"status":"mismatch","reason":"expression"}}}}}}"##
    );
    let (output, written, _) =
        run_with_output("tests-run-mixed-filter", script, list, &NEXTEST_RUN);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        written,
        "existing=1\ntests_run=1\ntests_listed=3\ntests_by_package=gone=0/2 kept=1/1\n"
    );
}

#[test]
fn the_required_gate_fails_when_the_filter_excluded_a_selected_package() {
    let script = r"cat >&2 <<'END'
    PASS [   0.012s] (1/1) kept alpha
 Summary [   0.012s] 1 test run: 1 passed, 0 skipped
END";
    let list = concat!(
        r##"{"test-count":3,"rust-suites":{"##,
        r##""kept":{"package-name":"kept","status":"listed","testcases":{"alpha":{"ignored":false,"filter-match":{"status":"matches"}}}},"##,
        r##""gone":{"package-name":"gone","status":"listed","testcases":{"one":{"ignored":false,"filter-match":{"status":"mismatch","reason":"expression"}},"two":{"ignored":false,"filter-match":{"status":"mismatch","reason":"expression"}}}}}}"##
    );
    let (_, written, _) = run_with_output("tests-run-mixed-gate", script, list, &NEXTEST_RUN);
    let outputs = written
        .lines()
        .filter_map(|line| line.split_once('='))
        .map(|(key, value)| (key.to_owned(), value.to_owned()))
        .collect::<BTreeMap<_, _>>();
    let spec = VerdictSpec::parse(
        "rust-plan",
        &[
            "rust-plan=always".to_owned(),
            "rust-test=lane:root".to_owned(),
        ],
        &["rust-test=root".to_owned()],
    )
    .expect("verdict spec");
    let needs = BTreeMap::from([
        (
            JobId::new("rust-plan").expect("job id"),
            Need {
                result: Some("success".to_owned()),
                outputs: BTreeMap::from([
                    ("rust_root_mode".to_owned(), "some".to_owned()),
                    ("rust_root_tests".to_owned(), "run".to_owned()),
                ]),
            },
        ),
        (
            JobId::new("rust-test").expect("job id"),
            Need {
                result: Some("success".to_owned()),
                outputs,
            },
        ),
    ]);
    let verdict = decide(&spec, &needs, Some("pull_request"));
    assert!(!verdict.passed, "{}", verdict.table());
    assert!(
        verdict
            .table()
            .contains("package gone of root listed 2 tests and ran none"),
        "{}",
        verdict.table()
    );
}

#[test]
fn a_package_whose_tests_are_all_ignored_does_not_fail_the_required_gate() {
    let script = r"cat >&2 <<'END'
     Summary [   0.012s] 0 tests run: 0 passed, 1 skipped
END";
    let list = concat!(
        r##"{"test-count":1,"rust-suites":{"##,
        r##""idle":{"package-name":"idle","status":"listed","testcases":{"hidden":{"ignored":true,"filter-match":{"status":"mismatch","reason":"ignored"}}}}}}"##
    );
    let (output, written, _) = run_with_output("tests-run-ignored", script, list, &NEXTEST_RUN);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        written,
        "existing=1\ntests_run=0\ntests_listed=0\ntests_by_package=\n"
    );
    let outputs = written
        .lines()
        .filter_map(|line| line.split_once('='))
        .map(|(key, value)| (key.to_owned(), value.to_owned()))
        .collect::<BTreeMap<_, _>>();
    let spec = VerdictSpec::parse(
        "rust-plan",
        &[
            "rust-plan=always".to_owned(),
            "rust-test=lane:root".to_owned(),
        ],
        &["rust-test=root".to_owned()],
    )
    .expect("verdict spec");
    let needs = BTreeMap::from([
        (
            JobId::new("rust-plan").expect("job id"),
            Need {
                result: Some("success".to_owned()),
                outputs: BTreeMap::from([
                    ("rust_root_mode".to_owned(), "some".to_owned()),
                    ("rust_root_tests".to_owned(), "run".to_owned()),
                ]),
            },
        ),
        (
            JobId::new("rust-test").expect("job id"),
            Need {
                result: Some("success".to_owned()),
                outputs,
            },
        ),
    ]);
    let verdict = decide(&spec, &needs, Some("pull_request"));
    assert!(verdict.passed, "{}", verdict.table());
}
