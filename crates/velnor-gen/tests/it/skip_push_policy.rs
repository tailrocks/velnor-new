// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::Value;
use velnor_model::{RepoPath, RepoView};

use crate::events::DENY_MISE;
use crate::facts::{TestResult, path};
use crate::pipeline::{view, workflow};

const KNOB: &str = "[ci]\nskip_push_after_merge_group = true\n";

fn repo(roots: &[&str], config: &str) -> Result<RepoView, Box<dyn std::error::Error>> {
    let mut repo = view(roots, Some(config.as_bytes()))?;
    let lock = path(".velnor/rust-ci.lock")?;
    let old = String::from_utf8(repo.files[&lock].clone())?;
    repo.files
        .insert(lock, old.replace("0.1.81", "0.1.102").into_bytes());
    for (name, text) in [("deny.toml", ""), ("mise.toml", DENY_MISE)] {
        let name = RepoPath::new(name)?;
        repo.tracked_paths.insert(name.clone());
        repo.files.insert(name, text.as_bytes().to_vec());
    }
    Ok(repo)
}

fn jobs(text: &str) -> Result<Value, Box<dyn std::error::Error>> {
    Ok(crate::yaml_tree::document(text)?["jobs"].clone())
}

fn condition(job: &Value, name: &str) -> String {
    job["steps"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|step| step["name"] == name)
        .and_then(|step| step["if"].as_str())
        .unwrap_or_default()
        .to_owned()
}

fn deny_condition(job: &Value) -> String {
    job["steps"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|step| {
            step["run"]
                .as_str()
                .is_some_and(|run| run.contains("cargo-deny"))
        })
        .and_then(|step| step["if"].as_str())
        .unwrap_or_default()
        .to_owned()
}

#[test]
fn the_policy_checks_skip_after_a_passed_queue_run_in_a_one_root_and_a_many_root_workflow()
-> TestResult {
    for (roots, producer) in [
        (&["Cargo.toml"][..], "rust"),
        (&["Cargo.toml", "lab/Cargo.toml"][..], "rust-plan"),
    ] {
        let all = jobs(&workflow(&repo(roots, KNOB)?)?)?;
        let policy = &all["policy"];
        let skip = format!("needs.{producer}.outputs.skip_push != 'true'");
        let when = deny_condition(policy);
        assert!(when.contains(&skip), "{producer}: {when}");
        let needs: Vec<&str> = policy["needs"]
            .as_array()
            .ok_or("needs")?
            .iter()
            .filter_map(Value::as_str)
            .collect();
        assert!(needs.contains(&producer), "{needs:?}");
    }
    Ok(())
}

#[test]
fn the_policy_job_of_a_one_root_workflow_runs_after_the_rust_job_failed_at_a_later_step()
-> TestResult {
    let all = jobs(&workflow(&repo(&["Cargo.toml"], "")?)?)?;
    let policy = all["policy"]["if"].as_str().ok_or("if")?;
    assert!(policy.contains("!cancelled()"), "{policy}");
    assert!(!policy.contains("needs.rust.result"), "{policy}");
    assert!(
        policy.contains("needs.rust.outputs.rust_deny == 'run'"),
        "{policy}"
    );
    let planned = jobs(&workflow(&repo(&["Cargo.toml", "lab/Cargo.toml"], "")?)?)?;
    let policy = planned["policy"]["if"].as_str().ok_or("if")?;
    assert!(
        policy.contains("needs.rust-plan.result == 'success'"),
        "{policy}"
    );
    Ok(())
}

#[test]
fn the_ask_step_names_the_required_check_whose_success_counts_in_a_running_queue_run() -> TestResult
{
    for (config, name) in [
        (KNOB.to_owned(), "Required"),
        (format!("required_check = \"Gate\"\n{KNOB}"), "Gate"),
    ] {
        let all = jobs(&workflow(&repo(&["Cargo.toml"], &config)?)?)?;
        let ask = all["rust"]["steps"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|step| step["name"] == "velnor-ci merge-group-passed")
            .ok_or("ask")?;
        assert_eq!(ask["env"]["VELNOR_REQUIRED_CHECK"], name);
    }
    Ok(())
}

#[test]
fn a_lane_that_planned_everything_again_runs_its_checks_whatever_the_queue_run_did() -> TestResult {
    let all = jobs(&workflow(&repo(&["Cargo.toml"], KNOB)?)?)?;
    let when = condition(&all["rust"], "Lint");
    assert!(
        when.contains(
            "(steps.merge_group.outputs.skip != 'true' || steps.replan.outcome == 'success')"
        ),
        "{when}"
    );
    Ok(())
}
