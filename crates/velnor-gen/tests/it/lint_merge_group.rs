// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::Value;
use velnor_model::{RepoPath, RepoView};

use crate::facts::TestResult;
use crate::lint_lane::{IDS, LINT, MISE};
use crate::pipeline::{view, workflow};

const SKIP: &str = "needs.rust-plan.outputs.skip_push != 'true'";

fn repo(roots: &[&str], knob: bool) -> Result<RepoView, Box<dyn std::error::Error>> {
    let config = format!(
        "{}{LINT}",
        if knob {
            "[ci]\nskip_push_after_merge_group = true\n\n"
        } else {
            ""
        }
    );
    let mut repo = view(roots, Some(config.as_bytes()))?;
    repo.files
        .insert(RepoPath::new("mise.toml")?, MISE.as_bytes().to_vec());
    let lock = RepoPath::new(".velnor/rust-ci.lock")?;
    let old = String::from_utf8(repo.files[&lock].clone())?;
    repo.files
        .insert(lock, old.replace("0.1.81", "0.1.88").into_bytes());
    Ok(repo)
}

fn jobs(roots: &[&str], knob: bool) -> Result<Value, Box<dyn std::error::Error>> {
    Ok(crate::yaml_tree::document(&workflow(&repo(roots, knob)?)?)?["jobs"].clone())
}

#[test]
fn planned_lint_jobs_wait_for_the_plan_and_skip_every_step_after_a_queue_run() -> TestResult {
    let all = jobs(&["Cargo.toml", "lab/Cargo.toml"], true)?;
    for id in IDS {
        let job = &all[id];
        assert_eq!(job["needs"], serde_json::json!(["rust-plan"]), "{id}");
        let steps = job["steps"].as_array().ok_or("steps")?;
        assert!(steps.len() >= 3, "{id}");
        for step in steps {
            let when = step["if"].as_str().unwrap_or_default();
            assert!(when.contains(SKIP), "{id} {step}");
        }
    }
    assert!(
        all["required"]["needs"]
            .as_array()
            .ok_or("needs")?
            .iter()
            .all(|need| all.get(need.as_str().unwrap_or_default()).is_some())
    );
    Ok(())
}

#[test]
fn lint_jobs_are_untouched_without_the_knob_and_in_a_single_job_workflow() -> TestResult {
    for (roots, knob) in [
        (&["Cargo.toml", "lab/Cargo.toml"][..], false),
        (&["Cargo.toml"][..], true),
    ] {
        let all = jobs(roots, knob)?;
        for id in IDS {
            let job = &all[id];
            assert!(!job.to_string().contains("skip_push"), "{id}: {job}");
            assert!(job["needs"].is_null(), "{id}: {job}");
        }
    }
    Ok(())
}
