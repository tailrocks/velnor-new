// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::error::Error;

use serde_json::Value;
use velnor_actions_workflow_renderer::render_schema2_workflows;

use super::job_condition_eval::evaluate;

fn jobs() -> Result<Value, Box<dyn Error>> {
    let files =
        render_schema2_workflows(&super::stable_sources_qualification_graph::request(true)?)?;
    let file = files
        .into_iter()
        .find(|file| file.path == ".github/workflows/qualification.yml")
        .ok_or("missing qualification workflow")?;
    let document: Value = serde_saphyr::from_str(&file.bytes)?;
    Ok(document["jobs"].clone())
}

fn running(jobs: &Value, mode: &str, git_ref: &str) -> Result<Vec<String>, Box<dyn Error>> {
    let context = BTreeMap::from([
        ("inputs.mode", mode),
        ("github.ref", git_ref),
        ("github.event_name", "workflow_dispatch"),
        ("github.repository", "tailrocks/velnor-new"),
        ("github.event.repository.default_branch", "main"),
    ]);
    let mut ids = Vec::new();
    for (id, job) in jobs.as_object().ok_or("jobs is not a map")? {
        let selected = match job.get("if").and_then(Value::as_str) {
            Some(condition) => evaluate(condition, &context)?,
            None => true,
        };
        if selected {
            ids.push(id.clone());
        }
    }
    Ok(ids)
}

fn exits_nonzero(jobs: &Value, id: &str) -> bool {
    jobs[id]["steps"].as_array().is_some_and(|steps| {
        steps.iter().any(|step| {
            step["run"]
                .as_str()
                .is_some_and(|run| run.trim_end().ends_with("exit 1"))
        })
    })
}

#[test]
fn a_dispatch_that_selects_no_qualification_job_fails_the_run() -> Result<(), Box<dyn Error>> {
    let jobs = jobs()?;
    for (mode, git_ref) in [
        ("stable-sources", "refs/heads/main"),
        ("stable-sources", "refs/tags/v1"),
        ("not-a-mode", "refs/heads/main"),
        ("not-a-mode", "refs/heads/feature"),
        ("", "refs/heads/feature"),
    ] {
        let ids = running(&jobs, mode, git_ref)?;
        assert!(!ids.is_empty(), "{mode} on {git_ref} skips every job");
        assert!(
            ids.iter().all(|id| exits_nonzero(&jobs, id)),
            "{mode} on {git_ref} runs {ids:?}, which must all fail the run"
        );
    }
    Ok(())
}

#[test]
fn a_dispatch_that_selects_real_jobs_does_not_run_the_mode_guard() -> Result<(), Box<dyn Error>> {
    let jobs = jobs()?;
    for (mode, git_ref) in [
        ("both", "refs/heads/main"),
        ("stable-sources", "refs/heads/feature"),
        ("pressure", "refs/heads/main"),
        ("cache", "refs/heads/feature"),
    ] {
        let ids = running(&jobs, mode, git_ref)?;
        assert!(!ids.is_empty(), "{mode} on {git_ref} runs nothing");
        assert!(
            !ids.iter().any(|id| exits_nonzero(&jobs, id)),
            "{mode} on {git_ref} runs a failing job: {ids:?}"
        );
    }
    let cache_on_default = running(&jobs, "cache", "refs/heads/main")?;
    assert!(cache_on_default.contains(&"cache-default-branch-guard".to_owned()));
    Ok(())
}
