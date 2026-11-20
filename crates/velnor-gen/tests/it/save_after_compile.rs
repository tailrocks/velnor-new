// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::base_save::{jobs, names, step, steps};
use crate::facts::TestResult;
use crate::gate_eval::{Context, condition_of, runs};

const RECORDING: &str = "0.1.107";
const RECORD: &str = "Record that the build can be saved";

const SAVES: [&str; 6] = [
    "velnor-ci inputs save",
    "Decide whether to save the build directory",
    "Save the target cache",
    "Record the target cache key",
    "Save the sources cache",
    "Record the sources cache key",
];

fn rust_lane(helper: &str) -> Result<String, Box<dyn std::error::Error>> {
    jobs(helper, "")?
        .into_iter()
        .find(|(id, _)| id == "rust")
        .map(|(_, lane)| lane)
        .ok_or_else(|| "no rust job".into())
}

fn compile_ids(lane: &str) -> Vec<String> {
    steps(lane)
        .into_iter()
        .filter(|step| {
            step.contains("inputs build -- cargo-clippy")
                || step.contains("inputs build -- cargo doc")
                || step.contains("inputs build -- cargo check")
        })
        .filter_map(|step| {
            step.lines()
                .find_map(|line| line.trim().strip_prefix("id: ").map(str::to_owned))
        })
        .collect()
}

fn pushed_to_main() -> Context {
    Context::new(&[
        ("github.ref", "refs/heads/main"),
        ("github.event.repository.default_branch", "main"),
        ("github.event_name", "push"),
        ("steps.restored.outcome", "success"),
        ("steps.plan.outcome", "success"),
        ("steps.plan.outputs.rust_root_mode", "full"),
        ("steps.replan.outcome", "skipped"),
        ("steps.base.outputs.save", "true"),
        ("steps.test.outputs.built", "true"),
    ])
}

fn saves(lane: &str, mut context: Context) -> Result<Vec<bool>, Box<dyn std::error::Error>> {
    let record = step(lane, RECORD).ok_or("no record step")?;
    let recorded = runs(&condition_of(record)?, &context)?;
    context.set(
        "steps.saveable.outcome",
        if recorded { "success" } else { "skipped" },
    );
    let mut ran = Vec::new();
    for name in &SAVES[..4] {
        let block = step(lane, name).ok_or_else(|| format!("no {name}"))?;
        let saved = runs(&condition_of(block)?, &context)?;
        if *name == SAVES[0] {
            context.set(
                "steps.inputs_save.outcome",
                if saved { "success" } else { "skipped" },
            );
        }
        ran.push(saved);
    }
    Ok(ran)
}

fn with_outcomes(ids: &[String], failed: Option<&str>, outcome: &str) -> Context {
    let mut context = pushed_to_main();
    for id in ids {
        let own = if Some(id.as_str()) == failed {
            "failure"
        } else {
            outcome
        };
        context.set(&format!("steps.{id}.outcome"), own);
    }
    context
}

#[test]
fn the_saves_read_one_step_that_names_every_compile_step_and_not_the_test_step() -> TestResult {
    let lane = rust_lane(RECORDING)?;
    let ids = compile_ids(&lane);
    assert!(ids.len() >= 2, "{ids:?}");
    let record = step(&lane, RECORD).ok_or("no record")?;
    for id in &ids {
        let term = format!("steps.{id}.outcome != ''failure''");
        assert!(record.contains(&term), "{term}: {record}");
    }
    assert!(!record.contains("steps.test.outcome"), "{record}");
    for name in SAVES {
        let saved = step(&lane, name).ok_or("no save")?;
        assert!(
            saved.contains("steps.saveable.outcome == ''success''"),
            "{name}: {saved}"
        );
        assert!(!saved.contains("steps.test.outcome"), "{name}: {saved}");
    }
    let order = names(&lane);
    let at = |name: &str| order.iter().position(|found| found == name);
    assert!(at(RECORD) < at(SAVES[0]));
    assert!(at("Documentation") < at(RECORD));
    Ok(())
}

#[test]
fn a_failed_compile_step_saves_nothing() -> TestResult {
    let lane = rust_lane(RECORDING)?;
    let ids = compile_ids(&lane);
    assert_ne!(ids.len(), 0);
    for failed in &ids {
        let mut context = with_outcomes(&ids, Some(failed), "success");
        context.job_ok = false;
        let saved = saves(&lane, context)?;
        assert!(saved.iter().all(|ran| !ran), "{failed}: {saved:?}");
    }
    Ok(())
}

#[test]
fn a_failed_test_after_green_compile_steps_still_saves() -> TestResult {
    let lane = rust_lane(RECORDING)?;
    let mut context = with_outcomes(&compile_ids(&lane), None, "success");
    context.set("steps.test.outcome", "failure");
    context.job_ok = false;
    let saved = saves(&lane, context)?;
    assert!(saved.iter().all(|ran| *ran), "{saved:?}");
    Ok(())
}

#[test]
fn a_compile_step_the_plan_left_out_does_not_stop_the_save() -> TestResult {
    let lane = rust_lane(RECORDING)?;
    let context = with_outcomes(&compile_ids(&lane), None, "skipped");
    let saved = saves(&lane, context)?;
    assert!(saved.iter().all(|ran| *ran), "{saved:?}");
    Ok(())
}

#[test]
fn a_job_with_its_own_build_step_needs_no_check_terms() -> TestResult {
    let lane = rust_lane("0.1.81")?;
    assert!(step(&lane, RECORD).is_none());
    let order = names(&lane);
    let build = order
        .iter()
        .position(|name| name == "Build the tests")
        .ok_or("no build")?;
    let build_step = step(&lane, "Build the tests").ok_or("no build step")?;
    assert!(!build_step.contains("!cancelled()"), "{build_step}");
    for check in ["Lint", "Documentation"] {
        let at = order.iter().position(|name| name == check);
        assert!(at < Some(build), "{check}");
    }
    Ok(())
}
