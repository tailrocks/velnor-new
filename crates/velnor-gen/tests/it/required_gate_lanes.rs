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
