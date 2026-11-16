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
        if *job == "rust-test"
            && let Some(count) = run.count
        {
            need.outputs
                .insert("tests_run".to_owned(), count.to_owned());
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
    for result in ["failure", "cancelled"] {
        let run = with(passing(), |run| set(&mut run.results, "rust-docs", result));
        expect(&run, false, "rust-docs was not scheduled by the plan")?;
    }
    Ok(())
}

#[test]
fn zero_tests_in_a_lane_whose_selected_packages_have_tests_fails() -> TestResult {
    let run = with(passing(), |run| run.count = Some("0"));
    expect(&run, false, "0 tests ran for root")?;
    let run = with(passing(), |run| {
        run.outputs.iter_mut().for_each(|(key, value)| {
            if *key == "rust_root_tests" {
                *value = "skip";
            }
        });
    });
    let run = with(run, |run| run.count = Some("0"));
    expect(&run, true, "Required: pass")
}

#[test]
fn a_missing_empty_or_garbled_tests_run_fails() -> TestResult {
    for bad in [
        None,
        Some(""),
        Some("-1"),
        Some("1.5"),
        Some("12 "),
        Some("0x10"),
        Some("+5"),
        Some("99999999999999999999999"),
    ] {
        let run = with(passing(), |run| run.count = bad);
        expect(
            &run,
            false,
            "tests_run is missing or not a number in a test job of root",
        )?;
    }
    Ok(())
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
    let words: Vec<String> = spec.arguments().split(' ').map(str::to_owned).collect();
    let mut jobs = Vec::new();
    let mut tests = Vec::new();
    let mut plan = String::new();
    for pair in words.chunks(2) {
        match pair[0].as_str() {
            "--plan" => plan.clone_from(&pair[1]),
            "--job" => jobs.push(pair[1].clone()),
            _ => tests.push(pair[1].clone()),
        }
    }
    assert_eq!(VerdictSpec::parse(&plan, &jobs, &tests)?, spec);
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
