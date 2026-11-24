// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::base_save::{jobs, steps};
use crate::facts::TestResult;
use crate::gate_eval::{Context, condition_of, runs};
use crate::pipeline::{pin_helper, view, workflow};

const EVERYTHING: &str = "on_main = \"everything\"\n";

fn audit_runs(
    helper: &str,
    config: &str,
    event: &str,
    expired: bool,
    inputs: &str,
) -> Result<bool, Box<dyn std::error::Error>> {
    audit_after_replan(helper, config, event, expired, inputs, "")
}

fn audit_after_replan(
    helper: &str,
    config: &str,
    event: &str,
    expired: bool,
    inputs: &str,
    replanned: &str,
) -> Result<bool, Box<dyn std::error::Error>> {
    let lane = jobs(helper, config)?
        .into_iter()
        .find(|(id, _)| id == "rust")
        .map(|(_, lane)| lane)
        .ok_or("no rust job")?;
    let audit = steps(&lane)
        .into_iter()
        .find(|step| step.contains("name: velnor-ci inputs audit"))
        .ok_or("no audit")?;
    let mut context = Context::new(&[
        ("steps.restored.outcome", "success"),
        ("steps.plan.outcome", "success"),
        ("steps.plan.outputs.rust_root_mode", "some"),
        ("steps.build_1.outcome", "success"),
        ("steps.test.outputs.built", "true"),
    ]);
    context.set("github.event_name", event);
    context.set("steps.restored.outputs.expired", &expired.to_string());
    context.set("steps.plan.outputs.rust_inputs", inputs);
    context.set("steps.replan.outputs.rust_inputs", replanned);
    context.set("steps.replan.outcome", "success");
    runs(&condition_of(audit)?, &context)
}

#[test]
fn a_pull_request_that_changed_a_lock_or_a_manifest_audits_inputs_from_helper_0_1_113() -> TestResult
{
    for config in ["", EVERYTHING] {
        assert!(
            audit_runs("0.1.116", config, "pull_request", false, "run")?,
            "{config}"
        );
    }
    Ok(())
}

#[test]
fn a_pull_request_that_changed_neither_does_not_audit_with_a_warm_cache() -> TestResult {
    for config in ["", EVERYTHING] {
        assert!(
            !audit_runs("0.1.116", config, "pull_request", false, "skip")?,
            "{config}"
        );
    }
    assert!(audit_runs("0.1.116", "", "pull_request", true, "skip")?);
    Ok(())
}

#[test]
fn an_older_helper_never_reads_the_inputs_output() -> TestResult {
    for config in ["", EVERYTHING] {
        assert!(
            !audit_runs("0.1.112", config, "pull_request", false, "run")?,
            "{config}"
        );
    }
    assert!(audit_runs("0.1.112", "", "pull_request", true, "run")?);
    Ok(())
}

#[test]
fn a_merge_group_audits_inputs_only_when_a_lock_or_a_manifest_changed_from_helper_0_1_113()
-> TestResult {
    for config in ["", EVERYTHING] {
        assert!(
            audit_runs("0.1.116", config, "merge_group", false, "run")?,
            "{config}"
        );
        assert!(
            !audit_runs("0.1.116", config, "merge_group", false, "skip")?,
            "{config}"
        );
        assert!(
            !audit_runs("0.1.112", config, "merge_group", false, "run")?,
            "{config}"
        );
    }
    Ok(())
}

#[test]
fn a_full_replan_does_not_hide_the_changed_inputs_of_a_pull_request() -> TestResult {
    for config in ["", EVERYTHING] {
        for event in ["pull_request", "merge_group"] {
            assert!(
                audit_after_replan("0.1.116", config, event, true, "run", "skip")?,
                "{config} {event}"
            );
            assert!(
                !audit_after_replan("0.1.116", config, event, false, "skip", "skip")?,
                "{config} {event}"
            );
        }
    }
    Ok(())
}

#[test]
fn a_push_keeps_its_audit_rule_at_every_helper() -> TestResult {
    for helper in ["0.1.115", "0.1.116"] {
        for inputs in ["run", "skip"] {
            assert!(
                !audit_runs(helper, "", "push", false, inputs)?,
                "{helper} affected"
            );
            assert!(
                audit_runs(helper, "", "push", true, inputs)?,
                "{helper} expired"
            );
            assert!(
                audit_runs(helper, EVERYTHING, "push", false, inputs)?,
                "{helper} everything"
            );
        }
    }
    Ok(())
}

const GROUPED: &str = "full_run = \"nightly\"\n[[workspace_groups]]\nname = \"tools\"\nroots = [\"tools/a\", \"tools/b\"]\n";
const ROOTS: &[&str] = &["Cargo.toml", "tools/a/Cargo.toml", "tools/b/Cargo.toml"];

fn grouped(version: &str) -> Result<String, Box<dyn std::error::Error>> {
    let mut view = view(ROOTS, Some(GROUPED.as_bytes()))?;
    pin_helper(&mut view, version)?;
    workflow(&view)
}

#[test]
fn a_plan_job_exposes_the_inputs_output_to_the_audits_of_its_lanes() -> TestResult {
    let text = grouped("0.1.116")?;
    assert!(
        text.contains("rust_inputs: '${{ steps.plan.outputs.rust_inputs }}'"),
        "{text}"
    );
    assert!(
        text.contains("needs.rust-plan.outputs.rust_inputs == ''run''"),
        "{text}"
    );
    assert!(!text.contains("replan.outputs.rust_inputs"), "{text}");
    let older = grouped("0.1.112")?;
    assert!(!older.contains("rust_inputs"), "{older}");
    Ok(())
}
