// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{jobs, steps};
use crate::facts::TestResult;
use crate::gate_eval::{Context, condition_of, runs};
use crate::pipeline::{pin_helper, view, workflow};

const EVERYTHING: &str = "on_main = \"everything\"\n";
const LABEL: &str = "pr_ci = \"label\"\n";

fn audit_runs(
    helper: &str,
    config: &str,
    event: &str,
    expired: bool,
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
    runs(&condition_of(audit)?, &context)
}

#[test]
fn a_pull_request_audits_inputs_with_a_warm_cache_from_helper_0_1_117() -> TestResult {
    for config in ["", EVERYTHING] {
        assert!(
            audit_runs("0.1.117", config, "pull_request", false)?,
            "{config}"
        );
        assert!(
            audit_runs("0.1.117", config, "pull_request", true)?,
            "{config}"
        );
    }
    Ok(())
}

#[test]
fn a_merge_group_audits_inputs_with_a_warm_cache_from_helper_0_1_117() -> TestResult {
    for config in ["", EVERYTHING, LABEL] {
        assert!(
            audit_runs("0.1.117", config, "merge_group", false)?,
            "{config}"
        );
    }
    Ok(())
}

#[test]
fn an_older_helper_audits_a_pull_request_or_merge_group_only_when_the_base_expired() -> TestResult {
    for config in ["", EVERYTHING] {
        for event in ["pull_request", "merge_group"] {
            assert!(
                !audit_runs("0.1.115", config, event, false)?,
                "{config} {event}"
            );
        }
    }
    assert!(audit_runs("0.1.115", "", "pull_request", true)?);
    Ok(())
}

#[test]
fn a_push_keeps_its_audit_rule_at_every_helper() -> TestResult {
    for helper in ["0.1.115", "0.1.116"] {
        assert!(!audit_runs(helper, "", "push", false)?, "{helper} affected");
        assert!(audit_runs(helper, "", "push", true)?, "{helper} expired");
        assert!(
            audit_runs(helper, EVERYTHING, "push", false)?,
            "{helper} everything"
        );
        for event in ["schedule", "workflow_dispatch"] {
            assert!(audit_runs(helper, "", event, false)?, "{helper} {event}");
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

fn audit_condition(text: &str) -> Vec<String> {
    text.lines()
        .scan(false, |after_audit, line| {
            if line.contains("name: velnor-ci inputs audit") {
                *after_audit = true;
                return Some(None);
            }
            if *after_audit && line.trim_start().starts_with("if:") {
                *after_audit = false;
                return Some(Some(line.trim().to_owned()));
            }
            Some(None)
        })
        .flatten()
        .collect()
}

#[test]
fn the_audits_of_a_plan_job_lane_run_on_a_pull_request_from_helper_0_1_117() -> TestResult {
    let audits = audit_condition(&grouped("0.1.117")?);
    assert_eq!(audits.len(), 2);
    for condition in &audits {
        assert!(
            condition.contains("github.event_name == ''pull_request''"),
            "{condition}"
        );
        assert!(
            condition.contains("github.event_name == ''merge_group''"),
            "{condition}"
        );
    }
    for condition in audit_condition(&grouped("0.1.115")?) {
        assert!(!condition.contains("pull_request"), "{condition}");
    }
    Ok(())
}
