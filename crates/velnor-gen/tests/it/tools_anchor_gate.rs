// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use serde_json::Value;

use crate::facts::TestResult;
use crate::pipeline::{pin_helper, view, workflow};

const HELPER: &str = "0.1.110";
const PLAN: &str = "rust-plan";
const NEEDS_PREFIX: &str = "needs.rust-plan.outputs.";
const STEPS_PREFIX: &str = "steps.plan.outputs.";

fn render(manifests: &[&str], config: &str) -> Result<Value, Box<dyn std::error::Error>> {
    let mut repo = view(manifests, Some(config.as_bytes()))?;
    pin_helper(&mut repo, HELPER)?;
    Ok(crate::yaml_tree::document(&workflow(&repo)?)?)
}

fn steps(job: &Value) -> Vec<Value> {
    job["steps"].as_array().cloned().unwrap_or_default()
}

fn condition(step: &Value) -> String {
    step["if"].as_str().unwrap_or_default().to_owned()
}

fn outputs_read(text: &str, prefix: &str) -> BTreeSet<String> {
    text.split(prefix)
        .skip(1)
        .map(|rest| {
            rest.chars()
                .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
                .collect()
        })
        .collect()
}

fn anchor_steps(yaml: &Value) -> Vec<Value> {
    steps(&yaml["jobs"][PLAN])
        .into_iter()
        .filter(|step| step["name"] == "Find the anchor" || step["name"] == "Read the anchor")
        .collect()
}

fn takes_plan_anchor(job: &Value) -> bool {
    steps(job).iter().any(|step| {
        step["env"]["VELNOR_TOOLS_ANCHOR"] == "${{ needs.rust-plan.outputs.tools_anchor }}"
    })
}

#[test]
fn the_anchor_lookup_runs_only_when_a_job_that_takes_the_anchor_can_run() -> TestResult {
    for config in ["mbx_shared_store = true\n", "mbx_shared_store = false\n"] {
        let yaml = render(&["Cargo.toml", "tools/Cargo.toml"], config)?;
        let anchor = anchor_steps(&yaml);
        assert_eq!(anchor.len(), 2, "{config:?}");
        let jobs = yaml["jobs"].as_object().ok_or("jobs missing")?;
        let mut consumers = 0;
        for (id, job) in jobs.iter().filter(|(id, _)| id.as_str() != PLAN) {
            if !takes_plan_anchor(job) {
                continue;
            }
            consumers += 1;
            let gate = outputs_read(&condition(job), NEEDS_PREFIX);
            assert!(!gate.is_empty(), "{config:?} {id}: {}", condition(job));
            for step in &anchor {
                let read = outputs_read(&condition(step), STEPS_PREFIX);
                assert!(
                    read.is_superset(&gate),
                    "{config:?} {id} gate {gate:?} not covered by {}",
                    condition(step)
                );
            }
        }
        assert!(consumers >= 2, "{config:?}");
    }
    Ok(())
}

#[test]
fn the_anchor_lookup_is_skipped_when_the_plan_selects_nothing() -> TestResult {
    let yaml = render(&["Cargo.toml", "tools/Cargo.toml"], "")?;
    for step in anchor_steps(&yaml) {
        let text = condition(&step);
        assert!(text.contains("steps.plan.outcome == 'success'"), "{text}");
        assert!(text.contains("_mode != 'none'"), "{text}");
    }
    Ok(())
}
