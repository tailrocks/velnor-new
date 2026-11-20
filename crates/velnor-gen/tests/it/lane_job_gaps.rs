// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::Value;
use velnor_model::{JobCondition, RepoPath, RepoView, VerdictSpec};

use crate::archive::jobs_of;
use crate::base_save::{step, steps};
use crate::facts::TestResult;
use crate::feature_lanes::lane_view;
use crate::gate_eval::{Context, condition_of, runs};
use crate::lint_lane::{IDS, LINT, MISE};
use crate::pipeline::{pin_helper, view, workflow};
use crate::save_after_compile::{RECORD, saves, with_outcomes};

const FIXED: &str = "0.1.110";
const BEFORE: &str = "0.1.109";
const LANE_ID: &str = "rust-lane-local-mode-root";
const LANE: &str = "[[test_lane]]\nroot = '.'\nfeatures = ['local-mode']\npackages = ['package-0']\nfilter = 'test(~local::) or binary(local_host_public)'\nlint = true\ndoc = true\njob = 'parallel'\n";
const LABEL: &str = "pr_ci = \"label\"\n";
const KNOB: &str = "[ci]\nskip_push_after_merge_group = true\n\n";
const FULL_RUN: &str = "ci:auto";
const SKIP: &str = "needs.rust-plan.outputs.skip_push != 'true'";

fn lane_workflow(helper: &str, config: &str) -> Result<String, Box<dyn std::error::Error>> {
    workflow(&lane_view(helper, config)?)
}

fn lane_block(helper: &str, config: &str) -> Result<String, Box<dyn std::error::Error>> {
    jobs_of(&lane_workflow(helper, config)?)
        .into_iter()
        .find(|(id, _)| id == LANE_ID)
        .map(|(_, block)| block)
        .ok_or_else(|| "no lane job".into())
}

fn compile_ids(block: &str) -> Vec<String> {
    steps(block)
        .into_iter()
        .filter(|step| step.contains("cargo-clippy") || step.contains("cargo doc"))
        .filter_map(|step| {
            step.lines()
                .find_map(|line| line.trim().strip_prefix("id: ").map(str::to_owned))
        })
        .collect()
}

fn job_condition(helper: &str, config: &str) -> Result<String, Box<dyn std::error::Error>> {
    let jobs = crate::yaml_tree::document(&lane_workflow(helper, config)?)?["jobs"].clone();
    Ok(jobs[LANE_ID]["if"]
        .as_str()
        .ok_or("no lane condition")?
        .to_owned())
}

fn step_condition(block: &str, name: &str) -> Result<String, Box<dyn std::error::Error>> {
    let found = step(block, name).ok_or_else(|| format!("no step {name}"))?;
    Ok(found
        .lines()
        .find_map(|line| line.trim().strip_prefix("if: "))
        .ok_or("no if")?
        .to_owned())
}

#[test]
fn a_lane_test_that_failed_after_its_build_still_saves_the_lane_cache() -> TestResult {
    let block = lane_block(FIXED, LANE)?;
    let ids = compile_ids(&block);
    assert_eq!(ids.len(), 2, "{ids:?}");
    let mut context = with_outcomes(&ids, None, "success");
    context.set("steps.test.outcome", "failure");
    context.job_ok = false;
    let saved = saves(&block, context)?;
    assert!(saved.iter().all(|ran| *ran), "{saved:?}");
    Ok(())
}

#[test]
fn a_failed_lane_compile_step_or_an_unfinished_lane_build_saves_nothing() -> TestResult {
    let block = lane_block(FIXED, LANE)?;
    let ids = compile_ids(&block);
    for failed in &ids {
        let mut context = with_outcomes(&ids, Some(failed), "success");
        context.job_ok = false;
        let saved = saves(&block, context)?;
        assert!(saved.iter().all(|ran| !ran), "{failed}: {saved:?}");
    }
    let mut unbuilt = with_outcomes(&ids, None, "success");
    unbuilt.set("steps.test.outputs.built", "");
    unbuilt.set("steps.test.outcome", "failure");
    unbuilt.job_ok = false;
    let saved = saves(&block, unbuilt)?;
    assert!(saved.iter().all(|ran| !ran), "{saved:?}");
    Ok(())
}

#[test]
fn the_lane_saves_read_the_built_output_of_the_lane_test_and_not_the_job_result() -> TestResult {
    let block = lane_block(FIXED, LANE)?;
    let record = step(&block, RECORD).ok_or("no record step")?;
    assert!(
        record.contains("steps.test.outputs.built == ''true''"),
        "{record}"
    );
    assert!(!record.contains("success()"), "{record}");
    let note = step_condition(&block, "Note the target cache saved after a test failure")?;
    let mut context = Context::new(&[
        ("steps.restored.outcome", "success"),
        ("steps.save_target_root.outcome", "success"),
        ("steps.test.outcome", "failure"),
    ]);
    context.job_ok = false;
    assert!(
        runs(&condition_of(&format!("if: {note}"))?, &context)?,
        "{note}"
    );
    Ok(())
}

#[test]
fn a_pin_before_0_1_110_keeps_the_job_result_gate_on_the_lane_saves() -> TestResult {
    let block = lane_block(BEFORE, LANE)?;
    let record = step(&block, RECORD).ok_or("no record step")?;
    assert!(record.contains("success()"), "{record}");
    assert!(!record.contains("outputs.built"), "{record}");
    Ok(())
}

#[test]
fn under_label_mode_the_lane_job_runs_unlabeled_with_its_test_and_guard_gated() -> TestResult {
    let config = format!("{LABEL}{LANE}");
    let job = job_condition(FIXED, &config)?;
    assert!(!job.contains(FULL_RUN), "{job}");
    let block = lane_block(FIXED, &config)?;
    for name in [
        "Test (features: local-mode)",
        "Test lane guard (features: local-mode)",
    ] {
        let when = step_condition(&block, name)?;
        assert!(when.contains(FULL_RUN), "{name}: {when}");
    }
    for name in [
        "Lint (features: local-mode)",
        "Documentation (features: local-mode)",
    ] {
        let when = step_condition(&block, name)?;
        assert!(!when.contains(FULL_RUN), "{name}: {when}");
    }
    Ok(())
}

#[test]
fn a_pin_before_0_1_110_keeps_the_label_gate_on_the_whole_lane_job() -> TestResult {
    let job = job_condition(BEFORE, &format!("{LABEL}{LANE}"))?;
    assert!(job.contains(FULL_RUN), "{job}");
    Ok(())
}

#[test]
fn without_label_mode_the_lane_job_condition_does_not_change() -> TestResult {
    let before = job_condition(BEFORE, LANE)?;
    let fixed = job_condition(FIXED, LANE)?;
    assert_eq!(before, fixed);
    Ok(())
}

fn planned(config: &str, helper: &str) -> Result<RepoView, Box<dyn std::error::Error>> {
    let mut repo = view(&["Cargo.toml", "lab/Cargo.toml"], Some(config.as_bytes()))?;
    pin_helper(&mut repo, helper)?;
    let mise = MISE.replace(
        "[tools]\n",
        "[tools]\n\"aqua:EmbarkStudios/cargo-deny\" = \"0.18.3\"\n",
    );
    for (name, text) in [("deny.toml", ""), ("mise.toml", mise.as_str())] {
        let name = RepoPath::new(name)?;
        repo.tracked_paths.insert(name.clone());
        repo.files.insert(name, text.as_bytes().to_vec());
    }
    Ok(repo)
}

fn jobs(config: &str, helper: &str) -> Result<Value, Box<dyn std::error::Error>> {
    Ok(crate::yaml_tree::document(&workflow(&planned(config, helper)?)?)?["jobs"].clone())
}

fn support_ids() -> Vec<&'static str> {
    IDS.iter().copied().chain(["policy"]).collect()
}

#[test]
fn the_policy_and_lint_jobs_are_not_allocated_after_a_skipped_push() -> TestResult {
    let config = format!("{KNOB}{LINT}");
    let all = jobs(&config, FIXED)?;
    for id in support_ids() {
        let job = &all[id];
        let when = job["if"].as_str().ok_or("no job condition")?;
        assert!(when.contains(SKIP), "{id}: {when}");
        assert!(
            job["needs"]
                .as_array()
                .ok_or("needs")?
                .iter()
                .any(|need| need == "rust-plan"),
            "{id}"
        );
    }
    Ok(())
}

#[test]
fn the_gate_treats_the_policy_and_lint_jobs_as_scheduled_unless_the_queue_run_passed() -> TestResult {
    let config = format!("{KNOB}{LINT}");
    let all = jobs(&config, FIXED)?;
    let text = all["required"]["steps"]
        .as_array()
        .into_iter()
        .flatten()
        .find_map(|step| step["env"]["GATE_SPEC"].as_str())
        .ok_or("no gate spec")?;
    let spec = VerdictSpec::from_text(text)?;
    for id in support_ids() {
        let job = spec
            .jobs
            .iter()
            .find(|job| job.id.as_str() == id)
            .ok_or_else(|| format!("the gate does not know {id}"))?;
        assert!(
            job.when.contains(&JobCondition::MergeGroupNotPassed),
            "{id}: {:?}",
            job.when
        );
    }
    Ok(())
}

#[test]
fn a_pin_before_0_1_110_keeps_the_support_jobs_allocated_and_their_steps_skipped() -> TestResult {
    let config = format!("{KNOB}{LINT}");
    let all = jobs(&config, "0.1.103")?;
    for id in support_ids() {
        let job = &all[id];
        let when = job["if"].as_str().unwrap_or_default();
        assert!(!when.contains("skip_push"), "{id}: {when}");
    }
    Ok(())
}

#[test]
fn without_the_knob_the_support_jobs_never_read_the_skip() -> TestResult {
    let all = jobs(LINT, FIXED)?;
    for id in support_ids() {
        assert!(!all[id].to_string().contains("skip_push"), "{id}");
    }
    Ok(())
}
