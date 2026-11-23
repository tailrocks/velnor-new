// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::base_save::{jobs, steps};
use crate::facts::TestResult;
use crate::gate_eval::{Context, condition_of, runs};

const EVERYTHING: &str = "on_main = \"everything\"\n";

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
        ("steps.plan.outputs.rust_root_mode", "full"),
        ("steps.build_1.outcome", "success"),
        ("steps.test.outputs.built", "true"),
    ]);
    context.set("github.event_name", event);
    context.set("steps.restored.outputs.expired", &expired.to_string());
    runs(&condition_of(audit)?, &context)
}

#[test]
fn a_pull_request_audits_inputs_with_a_warm_cache_from_helper_0_1_112() -> TestResult {
    for config in ["", EVERYTHING] {
        assert!(
            audit_runs("0.1.112", config, "pull_request", false)?,
            "{config}"
        );
    }
    Ok(())
}

#[test]
fn an_older_helper_audits_a_pull_request_only_when_the_base_expired() -> TestResult {
    for config in ["", EVERYTHING] {
        assert!(
            !audit_runs("0.1.111", config, "pull_request", false)?,
            "{config}"
        );
    }
    assert!(audit_runs("0.1.111", "", "pull_request", true)?);
    Ok(())
}

#[test]
fn a_push_keeps_its_audit_rule_at_every_helper() -> TestResult {
    for helper in ["0.1.111", "0.1.112"] {
        assert!(!audit_runs(helper, "", "push", false)?, "{helper} affected");
        assert!(audit_runs(helper, "", "push", true)?, "{helper} expired");
        assert!(
            audit_runs(helper, EVERYTHING, "push", false)?,
            "{helper} everything"
        );
    }
    Ok(())
}
