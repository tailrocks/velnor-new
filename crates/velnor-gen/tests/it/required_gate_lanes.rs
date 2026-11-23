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
            "rust-test-a=lane:root".to_owned(),
            "rust-test-b=lane:root".to_owned(),
            "pushed=schedule-or-push".to_owned(),
        ],
        &["rust-test-a=root".to_owned(), "rust-test-b=root".to_owned()],
    )
}

struct Run {
    mode: &'static str,
    lint: &'static str,
    pushed: &'static str,
    a: (&'static str, &'static str),
    b: (&'static str, &'static str),
    ignored: &'static str,
    event: &'static str,
}

fn passing() -> Run {
    Run {
        mode: "some",
        lint: "success",
        pushed: "skipped",
        a: ("4", "4"),
        b: ("2", "2"),
        ignored: "0",
        event: "pull_request",
    }
}

fn judge(run: &Run) -> Result<(bool, String), Box<dyn std::error::Error>> {
    let owned = |pairs: &[(&str, &str)]| -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    };
    let plan = owned(&[("rust_root_mode", run.mode), ("rust_root_tests", "run")]);
    let counts = |(tests_run, listed): (&str, &str)| {
        let mut outputs = owned(&[
            ("tests_run", tests_run),
            ("tests_listed", listed),
            ("tests_ignored", run.ignored),
        ]);
        if canonical(tests_run) && canonical(listed) {
            outputs.insert(
                "tests_by_package".to_owned(),
                format!("pkg={tests_run}/{listed}"),
            );
        }
        outputs
    };
    let mut needs = BTreeMap::new();
    for (job, result, outputs) in [
        ("rust-plan", "success", plan),
        ("rust-lint", run.lint, BTreeMap::new()),
        ("rust-test-a", "success", counts(run.a)),
        ("rust-test-b", "success", counts(run.b)),
        ("pushed", run.pushed, BTreeMap::new()),
    ] {
        needs.insert(
            JobId::new(job)?,
            Need {
                result: Some(result.to_owned()),
                outputs,
            },
        );
    }
    let verdict = decide(&spec()?, &needs, Some(run.event));
    Ok((verdict.passed, verdict.table()))
}

fn canonical(text: &str) -> bool {
    !(text.is_empty() || text.len() > 1 && text.starts_with('0'))
        && text.bytes().all(|byte| byte.is_ascii_digit())
}

fn expect(run: &Run, passes: bool, mention: &str) -> TestResult {
    let (green, table) = judge(run)?;
    assert_eq!(green, passes, "{mention}\n{table}");
    assert!(table.contains(mention), "{mention}\n{table}");
    Ok(())
}

#[test]
fn a_job_of_an_all_lane_is_scheduled_so_skipping_it_fails() -> TestResult {
    expect(&passing(), true, "Required: pass")?;
    let run = Run {
        mode: "all",
        lint: "skipped",
        ..passing()
    };
    expect(
        &run,
        false,
        "rust-lint was scheduled by the plan and its result is skipped",
    )?;
    let run = Run {
        mode: "all",
        ..passing()
    };
    expect(&run, true, "| rust-lint | scheduled | success |")
}

#[test]
fn a_schedule_or_push_job_is_scheduled_on_a_push_and_a_schedule_only() -> TestResult {
    expect(&passing(), true, "| pushed | not scheduled | skipped |")?;
    for event in ["push", "schedule"] {
        let run = Run { event, ..passing() };
        expect(
            &run,
            false,
            "pushed was scheduled by the plan and its result is skipped",
        )?;
        let run = Run {
            event,
            pushed: "success",
            ..passing()
        };
        expect(&run, true, "| pushed | scheduled | success |")?;
    }
    Ok(())
}

#[test]
fn a_package_split_across_the_lane_passes_when_the_lane_ran_it() -> TestResult {
    let run = Run {
        a: ("0", "3"),
        b: ("5", "5"),
        ..passing()
    };
    expect(&run, true, "Required: pass")?;
    let run = Run {
        a: ("5", "5"),
        b: ("0", "3"),
        ..passing()
    };
    expect(&run, true, "Required: pass")?;
    Ok(())
}

#[test]
fn a_split_lane_job_that_listed_and_ran_no_tests_fails_while_another_job_ran_some() -> TestResult {
    let run = Run {
        b: ("0", "0"),
        ..passing()
    };
    expect(
        &run,
        false,
        "a test job of root listed no tests and ran none",
    )
}

fn shard_needs(
    run: &str,
    listed: &str,
    map: Option<&str>,
    sibling_ran: bool,
) -> Result<BTreeMap<JobId, Need>, Box<dyn std::error::Error>> {
    let owned = |pairs: &[(&str, &str)]| -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    };
    let sibling = if sibling_ran {
        owned(&[
            ("tests_run", "4"),
            ("tests_listed", "4"),
            ("tests_ignored", "0"),
            ("tests_by_package", "pkg=4/4"),
        ])
    } else {
        let mut empty = owned(&[
            ("tests_run", "0"),
            ("tests_listed", "0"),
            ("tests_ignored", "0"),
        ]);
        empty.insert(
            "tests_by_package".to_owned(),
            map.unwrap_or("{}").to_owned(),
        );
        empty
    };
    let mut shard = owned(&[
        ("tests_run", run),
        ("tests_listed", listed),
        ("tests_ignored", "0"),
    ]);
    if let Some(map) = map {
        shard.insert("tests_by_package".to_owned(), map.to_owned());
    }
    Ok(BTreeMap::from([
        (
            JobId::new("rust-plan")?,
            Need {
                result: Some("success".to_owned()),
                outputs: owned(&[("rust_root_mode", "some"), ("rust_root_tests", "run")]),
            },
        ),
        (
            JobId::new("rust-lint")?,
            Need {
                result: Some("success".to_owned()),
                outputs: BTreeMap::new(),
            },
        ),
        (
            JobId::new("rust-test-a")?,
            Need {
                result: Some("success".to_owned()),
                outputs: sibling,
            },
        ),
        (
            JobId::new("rust-test-b")?,
            Need {
                result: Some("success".to_owned()),
                outputs: shard,
            },
        ),
        (
            JobId::new("pushed")?,
            Need {
                result: Some("skipped".to_owned()),
                outputs: BTreeMap::new(),
            },
        ),
    ]))
}

#[test]
fn a_shard_without_a_package_map_fails_even_while_another_shard_ran_some() -> TestResult {
    let needs = shard_needs("0", "0", None, true)?;
    let verdict = decide(&spec()?, &needs, Some("pull_request"));
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
fn a_shard_that_reports_the_empty_map_passes_while_another_shard_ran_some() -> TestResult {
    let needs = shard_needs("0", "0", Some("{}"), true)?;
    let verdict = decide(&spec()?, &needs, Some("pull_request"));
    assert!(verdict.passed, "{}", verdict.table());
    assert!(
        verdict.table().contains("Required: pass"),
        "{}",
        verdict.table()
    );
    Ok(())
}

#[test]
fn a_skipped_shard_with_the_empty_map_passes_while_another_shard_ran_some() -> TestResult {
    let mut needs = shard_needs("0", "0", Some("{}"), true)?;
    needs
        .get_mut(&JobId::new("rust-test-b")?)
        .ok_or("no shard")?
        .outputs
        .remove("tests_ignored");
    let verdict = decide(&spec()?, &needs, Some("pull_request"));
    assert!(verdict.passed, "{}", verdict.table());
    assert!(
        verdict.table().contains("Required: pass"),
        "{}",
        verdict.table()
    );
    Ok(())
}

#[test]
fn a_lane_where_every_shard_reports_no_tests_fails() -> TestResult {
    let needs = shard_needs("0", "0", Some("{}"), false)?;
    let verdict = decide(&spec()?, &needs, Some("pull_request"));
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

#[test]
fn a_lane_where_every_shard_is_skipped_fails_without_ignored_counts() -> TestResult {
    let mut needs = shard_needs("0", "0", Some("{}"), false)?;
    for job in ["rust-test-a", "rust-test-b"] {
        let id = JobId::new(job)?;
        let need = needs.get_mut(&id).ok_or("no shard")?;
        need.outputs.remove("tests_ignored");
    }
    let verdict = decide(&spec()?, &needs, Some("pull_request"));
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

#[test]
fn a_shard_that_ran_none_of_its_listed_tests_still_needs_its_package_map() -> TestResult {
    let needs = shard_needs("0", "3", None, true)?;
    let verdict = decide(&spec()?, &needs, Some("pull_request"));
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
fn a_package_that_listed_tests_and_the_lane_ran_none_fails() -> TestResult {
    let run = Run {
        a: ("0", "3"),
        b: ("0", "2"),
        ..passing()
    };
    expect(
        &run,
        false,
        "package pkg of root listed 3 tests and ran none",
    )?;
    let run = Run {
        a: ("0", "0"),
        b: ("0", "0"),
        ignored: "5",
        ..passing()
    };
    expect(&run, true, "Required: pass")?;
    let run = Run {
        a: ("4", "4"),
        b: ("", "2"),
        ..passing()
    };
    expect(
        &run,
        false,
        "tests_by_package is missing or malformed in a test job of root",
    )
}
