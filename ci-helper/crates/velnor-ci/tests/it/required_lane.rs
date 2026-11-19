// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_model::{JobId, VerdictSpec};
use velnor_rust::{Need, Verdict, decide};

const DEFAULT: &str = "rust-test";
const GROUP: &str = "rust-test-e2e";

fn job(id: &str, result: &str, packages: &str, run: u64, listed: u64) -> (JobId, Need) {
    (
        JobId::new(id).unwrap_or_else(|e| unreachable!("{e:?}")),
        Need {
            result: Some(result.to_owned()),
            outputs: BTreeMap::from([
                ("tests_by_package".to_owned(), packages.to_owned()),
                ("tests_run".to_owned(), run.to_string()),
                ("tests_listed".to_owned(), listed.to_string()),
            ]),
        },
    )
}

fn plan() -> (JobId, Need) {
    (
        JobId::new("rust-plan").unwrap_or_else(|e| unreachable!("{e:?}")),
        Need {
            result: Some("success".to_owned()),
            outputs: BTreeMap::from([
                ("rust_root_mode".to_owned(), "some".to_owned()),
                ("rust_root_tests".to_owned(), "run".to_owned()),
            ]),
        },
    )
}

fn gate(jobs: impl IntoIterator<Item = (JobId, Need)>) -> Verdict {
    let spec = VerdictSpec::parse(
        "rust-plan",
        &[
            "rust-plan=always".to_owned(),
            format!("{DEFAULT}=lane:root"),
            format!("{GROUP}=lane:root"),
        ],
        &[format!("{DEFAULT}=root"), format!("{GROUP}=root")],
    )
    .unwrap_or_else(|e| unreachable!("{e:?}"));
    let needs = std::iter::once(plan()).chain(jobs).collect();
    decide(&spec, &needs, Some("pull_request"))
}

#[test]
fn a_test_name_group_splitting_one_package_across_two_jobs_passes_the_gate() {
    let verdict = gate([
        job(DEFAULT, "success", "app=2/5 e2e=0/3 lib=4/4", 6, 12),
        job(GROUP, "success", "app=3/5 e2e=3/3 lib=0/4", 6, 12),
    ]);
    assert!(verdict.passed, "{}", verdict.table());
}

#[test]
fn a_package_whose_tests_ran_in_neither_job_fails_the_gate() {
    let verdict = gate([
        job(DEFAULT, "success", "app=0/5", 0, 5),
        job(GROUP, "success", "app=0/5", 0, 5),
    ]);
    assert!(!verdict.passed, "{}", verdict.table());
    assert!(
        verdict
            .table()
            .contains("package app of root listed 5 tests and ran none"),
        "{}",
        verdict.table()
    );
}

#[test]
fn a_package_filter_group_still_passes_when_each_package_ran_in_its_own_job() {
    let verdict = gate([
        job(DEFAULT, "success", "lib=4/4", 4, 4),
        job(GROUP, "success", "app=3/3", 3, 3),
    ]);
    assert!(verdict.passed, "{}", verdict.table());
}

#[test]
fn a_package_filter_group_still_fails_when_a_package_ran_in_no_job() {
    let verdict = gate([
        job(DEFAULT, "success", "lib=4/4", 4, 4),
        job(GROUP, "success", "app=0/3 lib=0/4", 0, 7),
    ]);
    assert!(!verdict.passed, "{}", verdict.table());
    assert!(
        verdict
            .table()
            .contains("package app of root listed 3 tests and ran none"),
        "{}",
        verdict.table()
    );
}

#[test]
fn a_missing_group_report_still_fails_the_gate() {
    let mut group = job(GROUP, "success", "app=3/5", 3, 5);
    group.1.outputs.remove("tests_by_package");
    let verdict = gate([job(DEFAULT, "success", "app=2/5", 2, 5), group]);
    assert!(!verdict.passed, "{}", verdict.table());
    assert!(
        verdict.table().contains("is missing or malformed"),
        "{}",
        verdict.table()
    );
}
