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

type Outputs = Vec<(&'static str, &'static str)>;

fn selected() -> Outputs {
    vec![
        ("rust_root_mode", "some"),
        ("rust_root_tests", "run"),
        ("rust_tools_mode", "none"),
        ("rust_deny", "skip"),
        ("rust_machete", "skip"),
    ]
}

struct Run {
    outputs: Outputs,
    results: Vec<(&'static str, &'static str)>,
    count: Option<&'static str>,
    listed: Option<&'static str>,
    packages: Option<String>,
    event: Option<&'static str>,
}

fn passing() -> Run {
    Run {
        outputs: selected(),
        results: vec![
            ("rust-plan", "success"),
            ("rust-lint", "success"),
            ("rust-test", "success"),
            ("rust-docs", "skipped"),
            ("policy", "skipped"),
            ("audit", "skipped"),
        ],
        count: Some("12"),
        listed: Some("12"),
        packages: None,
        event: Some("pull_request"),
    }
}

fn with(mut run: Run, edit: impl FnOnce(&mut Run)) -> Run {
    edit(&mut run);
    run
}

fn set(results: &mut Vec<(&'static str, &'static str)>, job: &'static str, result: &'static str) {
    results.retain(|(name, _)| *name != job);
    results.push((job, result));
}

fn judge(run: &Run) -> Result<(bool, String), Box<dyn std::error::Error>> {
    let mut needs = BTreeMap::new();
    for (job, result) in &run.results {
        let mut need = Need {
            result: Some((*result).to_owned()),
            outputs: BTreeMap::new(),
        };
        if *job == "rust-plan" {
            need.outputs = run
                .outputs
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect();
        }
        if *job == "rust-test" {
            for (name, value) in [("tests_run", run.count), ("tests_listed", run.listed)] {
                if let Some(value) = value {
                    need.outputs.insert(name.to_owned(), value.to_owned());
                }
            }
            if let Some(packages) = &run.packages {
                need.outputs
                    .insert("tests_by_package".to_owned(), packages.clone());
            } else if let (Some(count), Some(listed)) = (run.count, run.listed) {
                need.outputs.insert(
                    "tests_by_package".to_owned(),
                    format!("pkg={count}/{listed}"),
                );
            }
        }
        needs.insert(JobId::new(*job)?, need);
    }
    let verdict = decide(&spec()?, &needs, run.event);
    Ok((verdict.passed, verdict.table()))
}

fn expect(run: &Run, passes: bool, mention: &str) -> TestResult {
    let (green, table) = judge(run)?;
    assert_eq!(green, passes, "{mention}\n{table}");
    assert!(table.contains(mention), "{mention}\n{table}");
    Ok(())
}

#[test]
fn everything_the_plan_scheduled_passing_with_tests_run_passes() -> TestResult {
    expect(&passing(), true, "Required: pass")
}

#[test]
fn a_failed_or_missing_plan_fails() -> TestResult {
    for result in ["failure", "cancelled", "skipped"] {
        let run = with(passing(), |run| set(&mut run.results, "rust-plan", result));
        expect(&run, false, "the plan job rust-plan did not succeed")?;
    }
    let run = with(passing(), |run| {
        run.results.retain(|(job, _)| *job != "rust-plan");
    });
    expect(&run, false, "the plan job rust-plan is not in needs")
}

#[test]
fn a_missing_or_invalid_plan_output_fails() -> TestResult {
    let run = with(passing(), |run| {
        run.outputs.retain(|(key, _)| *key != "rust_root_mode");
    });
    expect(&run, false, "the plan output rust_root_mode is missing")?;
    for bad in ["", "everything", "NONE"] {
        let run = with(passing(), |run| {
            run.outputs.retain(|(key, _)| *key != "rust_root_mode");
            run.outputs.push(("rust_root_mode", bad));
        });
        expect(&run, false, "the plan output rust_root_mode is")?;
    }
    let run = with(passing(), |run| {
        run.outputs.retain(|(key, _)| *key != "rust_root_tests");
    });
    expect(&run, false, "the plan output rust_root_tests is missing")?;
    let run = with(passing(), |run| {
        run.outputs.retain(|(key, _)| *key != "rust_deny");
    });
    expect(&run, false, "the plan output rust_deny is missing")
}

#[test]
fn a_scheduled_job_that_did_not_succeed_fails() -> TestResult {
    for result in ["failure", "cancelled", "skipped", "timed_out"] {
        let run = with(passing(), |run| set(&mut run.results, "rust-lint", result));
        expect(&run, false, "rust-lint was scheduled by the plan")?;
    }
    Ok(())
}

#[test]
fn a_job_the_plan_did_not_schedule_may_be_skipped_and_a_success_is_a_warning() -> TestResult {
    expect(&passing(), true, "| rust-docs | not scheduled | skipped |")?;
    let run = with(passing(), |run| {
        set(&mut run.results, "rust-docs", "success");
    });
    expect(
        &run,
        true,
        "warning: rust-docs ran although the plan did not schedule it",
    )?;
    for result in [
        "failure",
        "cancelled",
        "timed_out",
        "neutral",
        "action_required",
    ] {
        let run = with(passing(), |run| set(&mut run.results, "rust-docs", result));
        expect(&run, false, "rust-docs was not scheduled by the plan")?;
    }
    Ok(())
}

#[test]
fn tests_that_were_listed_but_none_ran_fails_and_a_lane_with_no_tests_passes() -> TestResult {
    let run = with(passing(), |run| {
        run.count = Some("0");
        run.listed = Some("3");
    });
    expect(
        &run,
        false,
        "package pkg of root listed 3 tests and ran none",
    )?;
    let run = with(passing(), |run| {
        run.count = Some("0");
        run.listed = Some("0");
    });
    expect(&run, true, "Required: pass")?;
    let run = with(passing(), |run| {
        run.count = Some("2");
        run.listed = Some("9");
    });
    expect(&run, true, "Required: pass")?;
    let run = with(passing(), |run| {
        run.count = Some("2");
        run.listed = Some("6");
        run.packages = Some("other=0/4 ran=2/2".to_owned());
    });
    expect(
        &run,
        false,
        "package other of root listed 4 tests and ran none",
    )
}

#[test]
fn a_lane_whose_packages_have_no_test_targets_needs_no_count() -> TestResult {
    let run = with(passing(), |run| {
        run.outputs.iter_mut().for_each(|(key, value)| {
            if *key == "rust_root_tests" {
                *value = "skip";
            }
        });
        run.count = None;
        run.listed = None;
    });
    expect(&run, true, "Required: pass")
}

#[test]
fn a_missing_empty_or_garbled_tests_run_or_tests_listed_fails() -> TestResult {
    let garbled = [
        None,
        Some(""),
        Some("-1"),
        Some("1.5"),
        Some("12 "),
        Some("0x10"),
        Some("+5"),
        Some("99999999999999999999999"),
    ];
    for bad in garbled {
        let run = with(passing(), |run| run.count = bad);
        expect(
            &run,
            false,
            "tests_by_package is missing or malformed in a test job of root",
        )?;
        let run = with(passing(), |run| run.listed = bad);
        expect(
            &run,
            false,
            "tests_by_package is missing or malformed in a test job of root",
        )?;
    }
    let overlong = "p".repeat(40_000);
    for packages in [
        "other=0/4 ran=2/2",
        "ran=2/2",
        "pkg=2/1",
        "pkg=02/2",
        "pkg=2/2 extra",
        "-pkg=1/1",
        "pkg=1/1 pkg=1/1",
        "${{oops}}=1/1",
        "pkg=1/1 ",
        overlong.as_str(),
    ] {
        let run = with(passing(), |run| run.packages = Some(packages.to_owned()));
        expect(
            &run,
            false,
            "tests_by_package is missing or malformed in a test job of root",
        )?;
    }
    let run = with(passing(), |run| {
        run.count = Some("2");
        run.listed = Some("6");
        run.packages = Some("ran=2/2 other=0/4".to_owned());
    });
    expect(
        &run,
        false,
        "tests_by_package is missing or malformed in a test job of root",
    )
}

#[test]
fn a_job_missing_from_needs_or_unknown_to_the_gate_fails() -> TestResult {
    let run = with(passing(), |run| {
        run.results.retain(|(job, _)| *job != "rust-lint");
    });
    expect(&run, false, "rust-lint has no result in needs")?;
    let run = with(passing(), |run| {
        run.results.push(("added-later", "success"));
    });
    expect(
        &run,
        false,
        "added-later is in needs and the gate does not know it",
    )
}

#[test]
fn a_cancelled_run_never_passes() -> TestResult {
    let run = with(passing(), |run| {
        for (job, result) in &mut run.results {
            if *job != "rust-plan" && *result == "success" {
                *result = "cancelled";
            }
        }
    });
    expect(
        &run,
        false,
        "was scheduled by the plan and its result is cancelled",
    )
}

#[test]
fn a_schedule_only_job_must_run_on_a_schedule_and_may_be_skipped_otherwise() -> TestResult {
    let run = with(passing(), |run| run.event = Some("schedule"));
    expect(
        &run,
        false,
        "audit was scheduled by the plan and its result is skipped",
    )?;
    let run = with(run, |run| set(&mut run.results, "audit", "success"));
    expect(&run, true, "Required: pass")?;
    let run = with(passing(), |run| run.event = None);
    expect(&run, false, "GITHUB_EVENT_NAME is not set")
}

#[test]
fn the_policy_job_is_scheduled_when_deny_or_machete_runs() -> TestResult {
    let run = with(passing(), |run| {
        run.outputs.retain(|(key, _)| *key != "rust_machete");
        run.outputs.push(("rust_machete", "run"));
    });
    expect(
        &run,
        false,
        "policy was scheduled by the plan and its result is skipped",
    )?;
    let run = with(run, |run| set(&mut run.results, "policy", "success"));
    expect(&run, true, "Required: pass")
}

#[test]
fn the_spec_survives_its_own_command_line_and_refuses_malformed_arguments() -> TestResult {
    let spec = spec()?;
    assert_eq!(VerdictSpec::from_text(&spec.text())?, spec);
    assert!(VerdictSpec::from_text("--job a=always").is_err());
    assert!(VerdictSpec::from_text("--plan a --plan b").is_err());
    assert!(VerdictSpec::from_text("--plan").is_err());
    assert!(VerdictSpec::from_text("--plan a --other b").is_err());
    for (jobs, tests) in [
        (vec!["a"], vec![]),
        (vec!["a=never"], vec![]),
        (vec!["a=lane:Bad"], vec![]),
        (vec!["a=always", "a=always"], vec![]),
        (vec!["a=always"], vec!["b=root"]),
        (vec!["a=always"], vec!["a=Root"]),
    ] {
        let jobs: Vec<String> = jobs.into_iter().map(str::to_owned).collect();
        let tests: Vec<String> = tests.into_iter().map(str::to_owned).collect();
        assert!(
            VerdictSpec::parse("rust-plan", &jobs, &tests).is_err(),
            "{jobs:?} {tests:?}"
        );
    }
    Ok(())
}
