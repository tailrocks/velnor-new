// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_model::{JobId, VerdictSpec};
use velnor_rust::{Need, decide};

use crate::facts::TestResult;

fn spec() -> Result<VerdictSpec, String> {
    VerdictSpec::parse(
        "rust-plan",
        &[
            "rust-plan=always".to_owned(),
            "rust-lint=lane:root".to_owned(),
            "rust-test=lane:root".to_owned(),
            "rust-docs=lane:tools".to_owned(),
            "policy=policy".to_owned(),
            "audit=schedule".to_owned(),
        ],
        &["rust-test=root".to_owned()],
    )
}

fn selected() -> Vec<(&'static str, &'static str)> {
    vec![
        ("rust_root_mode", "some"),
        ("rust_root_tests", "run"),
        ("rust_tools_mode", "none"),
        ("rust_deny", "skip"),
        ("rust_machete", "skip"),
    ]
}

fn lone_needs(map: Option<&str>) -> Result<BTreeMap<JobId, Need>, Box<dyn std::error::Error>> {
    let mut needs = BTreeMap::new();
    needs.insert(
        JobId::new("rust-plan")?,
        Need {
            result: Some("success".to_owned()),
            outputs: selected()
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
        },
    );
    for (job, result) in [
        ("rust-lint", "success"),
        ("rust-test", "success"),
        ("rust-docs", "skipped"),
        ("policy", "skipped"),
        ("audit", "skipped"),
    ] {
        let mut outputs = BTreeMap::new();
        if job == "rust-test" {
            outputs.insert("tests_run".to_owned(), "0".to_owned());
            outputs.insert("tests_listed".to_owned(), "0".to_owned());
            outputs.insert("tests_ignored".to_owned(), "0".to_owned());
            if let Some(map) = map {
                outputs.insert("tests_by_package".to_owned(), map.to_owned());
            }
        }
        needs.insert(
            JobId::new(job)?,
            Need {
                result: Some(result.to_owned()),
                outputs,
            },
        );
    }
    Ok(needs)
}

#[test]
fn a_lone_job_without_a_package_map_fails_as_missing_evidence() -> TestResult {
    let verdict = decide(&spec()?, &lone_needs(None)?, Some("pull_request"));
    assert!(!verdict.passed, "{}", verdict.table());
    assert!(
        verdict
            .table()
            .contains("tests_by_package is missing or malformed in a test job of root"),
        "{}",
        verdict.table()
    );
    Ok(())
}

#[test]
fn a_lone_job_that_reports_the_empty_map_fails_as_a_zero_test_lane() -> TestResult {
    let verdict = decide(&spec()?, &lone_needs(Some("{}"))?, Some("pull_request"));
    assert!(!verdict.passed, "{}", verdict.table());
    assert!(
        verdict
            .table()
            .contains("a test job of root listed no tests and ran none"),
        "{}",
        verdict.table()
    );
    Ok(())
}
