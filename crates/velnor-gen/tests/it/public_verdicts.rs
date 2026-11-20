// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::Value;

use crate::facts::TestResult;
use crate::pipeline::workflow;
use crate::public_sources::{DECLARED, locked, pinned_source};

const HELPER: &str = "0.1.109";
const PARALLEL: &str = "[[test_lane]]\nroot = '.'\nfeatures = ['local-mode']\njob = 'parallel'\n";
const REGISTRY: &str = "sparse+https://index.crates.io/";
const TOKEN: &str = "${{ github.token }}";

fn jobs(source: &str, config: &str) -> Result<(Value, String), Box<dyn std::error::Error>> {
    let text = workflow(&locked(source, config, HELPER)?)?;
    Ok((crate::yaml_tree::document(&text)?["jobs"].clone(), text))
}

fn step<'a>(job: &'a Value, id: &str) -> Option<&'a Value> {
    job["steps"]
        .as_array()?
        .iter()
        .find(|step| step["id"] == id)
}

fn token_steps(jobs: &Value) -> Vec<String> {
    let mut found = Vec::new();
    for (job_id, job) in jobs.as_object().into_iter().flatten() {
        for step in job["steps"].as_array().into_iter().flatten() {
            if step["env"]["GITHUB_TOKEN"] == TOKEN {
                found.push(format!("{job_id}/{}", step["name"].as_str().unwrap_or("")));
            }
        }
    }
    found
}

#[test]
fn a_single_job_looks_the_repositories_up_once_and_its_decide_step_reads_that_step() -> TestResult {
    let (jobs, _) = jobs(&pinned_source(), DECLARED)?;
    let rust = &jobs["rust"];
    let lookup = step(rust, "public_sources").ok_or("no lookup step")?;
    assert!(
        lookup["run"]
            .as_str()
            .is_some_and(|run| run.ends_with(" cache public-sources")),
        "{lookup}"
    );
    assert_eq!(lookup["continue-on-error"], true);
    assert_eq!(lookup["env"]["GITHUB_TOKEN"], TOKEN);
    let condition = lookup["if"].as_str().ok_or("lookup is unconditional")?;
    assert!(
        condition.contains("github.event_name == 'push'") && condition.contains("default_branch"),
        "{condition}"
    );
    assert!(
        condition.contains("steps.plan.outputs.rust_") && condition.contains("_mode != 'none'"),
        "the lookup runs although no lane is selected: {condition}"
    );
    let decide = step(rust, "base").ok_or("no decide step")?;
    assert!(decide["env"]["GITHUB_TOKEN"].is_null(), "{decide}");
    assert_eq!(
        decide["env"]["VELNOR_PUBLIC_VERDICTS"],
        "${{ steps.public_sources.outputs.verdicts }}"
    );
    let order: Vec<&str> = rust["steps"]
        .as_array()
        .ok_or("no steps")?
        .iter()
        .filter_map(|step| step["id"].as_str())
        .collect();
    let at = |id: &str| order.iter().position(|found| *found == id);
    assert!(
        at("plan") < at("public_sources") && at("public_sources") < at("base"),
        "{order:?}"
    );
    Ok(())
}

#[test]
fn with_a_plan_job_one_lookup_serves_every_lane_through_a_job_output() -> TestResult {
    let (jobs, _) = jobs(&pinned_source(), &format!("{DECLARED}{PARALLEL}"))?;
    let plan = &jobs["rust-plan"];
    let lookup = step(plan, "public_sources").ok_or("no lookup step")?;
    let condition = lookup["if"].as_str().ok_or("lookup is unconditional")?;
    assert!(
        condition.contains("steps.plan.outputs.rust_") && condition.contains("_mode != 'none'"),
        "the lookup runs although no lane is selected: {condition}"
    );
    assert_eq!(
        plan["outputs"]["public_verdicts"],
        "${{ steps.public_sources.outputs.verdicts }}"
    );
    let mut deciding = 0;
    for (id, job) in jobs.as_object().ok_or("no jobs")? {
        if id == "rust-plan" {
            continue;
        }
        assert!(step(job, "public_sources").is_none(), "{id} looks up again");
        if let Some(decide) = step(job, "base") {
            deciding += 1;
            assert_eq!(
                decide["env"]["VELNOR_PUBLIC_VERDICTS"],
                "${{ needs.rust-plan.outputs.public_verdicts }}",
                "{id}"
            );
            assert!(decide["env"]["GITHUB_TOKEN"].is_null(), "{id}");
            assert!(
                job["needs"]
                    .as_array()
                    .is_some_and(|needs| needs.iter().any(|need| need == "rust-plan")),
                "{id} reads a job it does not need"
            );
        }
    }
    assert!(deciding > 0, "no lane decides");
    Ok(())
}

#[test]
fn the_token_is_on_the_lookup_step_only() -> TestResult {
    let (jobs, _) = jobs(&pinned_source(), &format!("{DECLARED}{PARALLEL}"))?;
    let (plain, _) = jobs_without_git()?;
    let with_git = token_steps(&jobs);
    let before = token_steps(&plain);
    let added: Vec<&String> = with_git
        .iter()
        .filter(|name| !before.contains(name))
        .collect();
    assert!(!added.is_empty(), "{with_git:?}");
    for name in &added {
        assert!(
            name.ends_with("Look up the public git dependencies once for this run"),
            "{name}"
        );
    }
    Ok(())
}

fn jobs_without_git() -> Result<(Value, String), Box<dyn std::error::Error>> {
    jobs(REGISTRY, PARALLEL)
}

#[test]
fn the_job_permissions_do_not_change() -> TestResult {
    let (with_git, _) = jobs(&pinned_source(), &format!("{DECLARED}{PARALLEL}"))?;
    let (plain, _) = jobs_without_git()?;
    let permissions = |jobs: &Value| -> Vec<(String, Value)> {
        jobs.as_object()
            .into_iter()
            .flatten()
            .map(|(id, job)| (id.clone(), job["permissions"].clone()))
            .collect()
    };
    assert_eq!(permissions(&with_git), permissions(&plain));
    for (id, permissions) in permissions(&with_git) {
        assert_ne!(permissions["contents"], "write", "{id}");
    }
    Ok(())
}

#[test]
fn a_lock_without_git_dependencies_gets_no_lookup_and_no_token_on_decide() -> TestResult {
    let (jobs, _) = jobs(REGISTRY, "")?;
    for job in jobs.as_object().ok_or("no jobs")?.values() {
        assert!(step(job, "public_sources").is_none());
        if let Some(decide) = step(job, "base") {
            assert!(decide["env"]["GITHUB_TOKEN"].is_null(), "{decide}");
            assert!(
                decide["env"]["VELNOR_PUBLIC_VERDICTS"].is_null(),
                "{decide}"
            );
        }
    }
    Ok(())
}
