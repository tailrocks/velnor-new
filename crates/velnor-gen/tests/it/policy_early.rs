// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{RepoPath, RepoView};

use crate::events::DENY_MISE;
use crate::facts::TestResult;
use crate::pipeline::{pin_helper, view, workflow};

const SKIP_PUSH: &[u8] = b"[ci]\nskip_push_after_merge_group = true\n";

fn repo(
    roots: &[&str],
    helper: &str,
    config: Option<&[u8]>,
) -> Result<RepoView, Box<dyn std::error::Error>> {
    let mut repo = view(roots, config)?;
    for (path, text) in [("deny.toml", ""), ("mise.toml", DENY_MISE)] {
        let path = RepoPath::new(path)?;
        repo.tracked_paths.insert(path.clone());
        repo.files.insert(path, text.as_bytes().to_vec());
    }
    pin_helper(&mut repo, helper)?;
    Ok(repo)
}

fn job(text: &str, id: &str) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    crate::yaml_tree::document(text)?
        .pointer(&format!("/jobs/{id}"))
        .cloned()
        .ok_or_else(|| format!("no job {id}").into())
}

fn needs(job: &serde_json::Value) -> Vec<&str> {
    job["needs"]
        .as_array()
        .map(|needs| needs.iter().filter_map(serde_json::Value::as_str).collect())
        .unwrap_or_default()
}

fn conditions(job: &serde_json::Value) -> String {
    job["steps"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|step| step["if"].as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

fn plans(job: &serde_json::Value) -> bool {
    job["steps"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|step| step["id"].as_str() == Some("plan"))
}

#[test]
fn the_one_job_policy_lane_does_not_wait_for_the_rust_job_on_the_first_release_that_plans_alone()
-> TestResult {
    let text = workflow(&repo(&["Cargo.toml"], "0.1.110", None)?)?;
    let policy = job(&text, "policy")?;
    assert!(needs(&policy).is_empty(), "{policy}");
    assert!(plans(&policy), "{policy}");
    let steps = conditions(&policy);
    assert!(
        steps.contains("steps.plan.outputs.rust_deny == 'run'"),
        "{steps}"
    );
    assert!(!steps.contains("needs.rust"), "{steps}");
    assert!(
        !policy["if"]
            .as_str()
            .unwrap_or_default()
            .contains("needs.rust"),
        "{policy}"
    );
    let required = job(&text, "required")?;
    let required = needs(&required);
    assert!(
        required.contains(&"rust") && required.contains(&"policy"),
        "{required:?}"
    );
    Ok(())
}

#[test]
fn an_older_pin_still_renders_the_policy_lane_after_the_rust_job() -> TestResult {
    for helper in ["0.1.81", "0.1.109"] {
        let text = workflow(&repo(&["Cargo.toml"], helper, None)?)?;
        let policy = job(&text, "policy")?;
        assert_eq!(needs(&policy), ["rust"], "{helper}: {policy}");
        assert!(!plans(&policy), "{helper}: {policy}");
        assert!(
            conditions(&policy).contains("needs.rust.outputs.rust_deny == 'run'"),
            "{helper}: {policy}"
        );
    }
    Ok(())
}

#[test]
fn the_early_policy_lane_still_skips_a_push_the_queue_run_already_passed() -> TestResult {
    let text = workflow(&repo(&["Cargo.toml"], "0.1.110", Some(SKIP_PUSH))?)?;
    let policy = job(&text, "policy")?;
    assert!(needs(&policy).is_empty(), "{policy}");
    let steps = conditions(&policy);
    assert!(
        steps.contains("steps.merge_group.outputs.skip != 'true'"),
        "{steps}"
    );
    assert!(!steps.contains("needs.rust.outputs.skip_push"), "{steps}");
    Ok(())
}

#[test]
fn the_multi_job_shape_keeps_policy_on_the_plan_job_at_every_pin() -> TestResult {
    for helper in ["0.1.109", "0.1.110"] {
        let text = workflow(&repo(&["Cargo.toml", "lab/Cargo.toml"], helper, None)?)?;
        assert_eq!(needs(&job(&text, "policy")?), ["rust-plan"], "{helper}");
    }
    Ok(())
}
