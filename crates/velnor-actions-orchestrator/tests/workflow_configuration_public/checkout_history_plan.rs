// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::error::Error;
use std::fs;
use std::path::Path;

use serde_json::{Value, json};
use tempfile::TempDir;
use velnor_actions_orchestrator::plan_internal;

use super::workflow_configuration_fixture::{self, git_fixture};

struct Origin {
    dir: TempDir,
    first: String,
    second: String,
    merge: String,
    pr_head: String,
}

fn git_output(cwd: &Path, args: &[&str]) -> Result<String, Box<dyn Error>> {
    let output = git_fixture::command(cwd)?.args(args).output()?;
    if !output.status.success() {
        return Err(format!(
            "git {args:?} failed in {}: {}",
            cwd.display(),
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}

fn git_outside(args: &[&str], cwd: &Path) -> Result<(), Box<dyn Error>> {
    let output = git_fixture::sterile_command()
        .args(args)
        .current_dir(cwd)
        .output()?;
    if !output.status.success() {
        return Err(format!(
            "git {args:?} failed in {}: {}",
            cwd.display(),
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    Ok(())
}

fn commit_all(root: &Path, message: &str) -> Result<String, Box<dyn Error>> {
    workflow_configuration_fixture::git(&["add", "-A"], root)?;
    workflow_configuration_fixture::git(&["commit", "-q", "-m", message], root)?;
    git_output(root, &["rev-parse", "HEAD"])
}

fn origin() -> Result<Origin, Box<dyn Error>> {
    let config = workflow_configuration_fixture::config_with_verification_branch(None);
    let dir = workflow_configuration_fixture::make_repo(&config)?;
    let root = dir.path();
    fs::write(
        root.join("mise.toml"),
        format!(
            "[tools]\nrust = \"{}\"\n",
            velnor_actions_mise::RUST_VERSION
        ),
    )?;
    for (relative_root, package_name) in [("crates/a-b", "a-b"), ("crates/a/b", "b")] {
        workflow_configuration_fixture::add_independent_mbx_package(
            root,
            relative_root,
            package_name,
        )?;
    }
    let first = commit_all(root, "base")?;
    fs::write(
        root.join("crates/a-b/src/lib.rs"),
        "pub fn f() -> u8 { 1 }\n",
    )?;
    let second = commit_all(root, "change a-b")?;
    workflow_configuration_fixture::git(&["checkout", "-q", "-b", "pr", &first], root)?;
    fs::write(
        root.join("crates/a/b/src/lib.rs"),
        "pub fn f() -> u8 { 2 }\n",
    )?;
    let pr_head = commit_all(root, "change b")?;
    workflow_configuration_fixture::git(&["checkout", "-q", "testmain"], root)?;
    workflow_configuration_fixture::git(&["merge", "-q", "--no-ff", "pr", "-m", "merge"], root)?;
    let merge = git_output(root, &["rev-parse", "HEAD"])?;
    Ok(Origin {
        dir,
        first,
        second,
        merge,
        pr_head,
    })
}

fn origin_url(origin: &Origin) -> String {
    format!("file://{}", origin.dir.path().display())
}

fn full_clone(origin: &Origin, target: &Path, checkout: &str) -> Result<(), Box<dyn Error>> {
    let parent = target.parent().ok_or("clone target has no parent")?;
    git_outside(
        &[
            "clone",
            "-q",
            &origin_url(origin),
            &target.to_string_lossy(),
        ],
        parent,
    )?;
    git_outside(&["checkout", "-q", "--detach", checkout], target)
}

fn shallow_clone(origin: &Origin, target: &Path, checkout: &str) -> Result<(), Box<dyn Error>> {
    fs::create_dir_all(target)?;
    git_outside(&["init", "-q", "-b", "testmain"], target)?;
    git_outside(
        &[
            "fetch",
            "-q",
            "--depth=1",
            "--no-tags",
            &origin_url(origin),
            &format!("+{checkout}:refs/remotes/origin/main"),
        ],
        target,
    )?;
    git_outside(&["checkout", "-q", "--detach", checkout], target)
}

fn is_shallow(root: &Path) -> Result<bool, Box<dyn Error>> {
    Ok(git_output(root, &["rev-parse", "--is-shallow-repository"])? == "true")
}

fn parent_resolves(root: &Path, parent: &str) -> bool {
    git_output(root, &["rev-parse", "--verify", "--quiet", parent]).is_ok()
}

fn plan(root: &Path, event: &str, base: &str, head: &str) -> Result<Value, Box<dyn Error>> {
    let request = json!({
        "schema": 1,
        "op": "plan-v1",
        "run_key": "r190001-a1",
        "base": base,
        "head": head,
        "event": event,
        "root": root.to_string_lossy(),
    });
    match plan_internal(&request.to_string()) {
        Ok(response) => Ok(serde_json::from_str::<Value>(&response)?["plan"].clone()),
        Err(error) => Err(format!("{event} planning failed: {error}").into()),
    }
}

fn dispositions(plan: &Value) -> Result<BTreeMap<String, String>, Box<dyn Error>> {
    plan["jobs"]
        .as_array()
        .ok_or("plan has no jobs")?
        .iter()
        .map(|job| {
            Ok((
                job["job_id"].as_str().ok_or("job has no id")?.to_owned(),
                job["disposition"]
                    .as_str()
                    .ok_or("job has no disposition")?
                    .to_owned(),
            ))
        })
        .collect()
}

fn warnings(plan: &Value) -> Vec<String> {
    plan["warnings"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|warning| warning.as_str().map(str::to_owned))
        .collect()
}

#[test]
fn push_plan_runs_every_job_identically_on_a_depth_one_checkout() -> Result<(), Box<dyn Error>> {
    let origin = origin()?;
    let scratch = tempfile::tempdir()?;
    let work = scratch.path().join("work");

    full_clone(&origin, &work, &origin.second)?;
    assert!(!is_shallow(&work)?);
    assert!(parent_resolves(&work, "HEAD^"));
    let full_plan = plan(&work, "push", &origin.first, &origin.second)?;
    fs::remove_dir_all(&work)?;

    shallow_clone(&origin, &work, &origin.second)?;
    assert!(is_shallow(&work)?);
    assert!(!parent_resolves(&work, "HEAD^"));
    let shallow_plan = plan(&work, "push", &origin.first, &origin.second)?;

    assert_eq!(
        serde_json::to_string_pretty(&full_plan)?,
        serde_json::to_string_pretty(&shallow_plan)?,
        "a depth-1 push plan must equal the full-history push plan"
    );
    let jobs = dispositions(&shallow_plan)?;
    assert!(!jobs.is_empty());
    assert!(
        jobs.values().all(|disposition| disposition == "run"),
        "push must run every job although only one crate changed: {jobs:?}"
    );
    let warnings = warnings(&shallow_plan);
    assert!(
        warnings
            .iter()
            .any(|warning| warning == "full_run_event:push:all_changed"),
        "push must plan from the event, not from history: {warnings:?}"
    );
    assert!(
        !warnings
            .iter()
            .any(|warning| warning.starts_with("comparison_unavailable")),
        "push must not attempt a comparison: {warnings:?}"
    );
    Ok(())
}

fn has_commit(root: &Path, sha: &str) -> bool {
    git_output(root, &["cat-file", "-e", &format!("{sha}^{{commit}}")]).is_ok()
}

#[test]
fn pull_request_plan_on_a_shallow_merge_checkout_fails_instead_of_narrowing()
-> Result<(), Box<dyn Error>> {
    let origin = origin()?;
    let full_scratch = tempfile::tempdir()?;
    let full = full_scratch.path().join("work");
    full_clone(&origin, &full, &origin.merge)?;
    let shallow_scratch = tempfile::tempdir()?;
    let shallow = shallow_scratch.path().join("work");
    shallow_clone(&origin, &shallow, &origin.merge)?;
    assert!(is_shallow(&shallow)?);
    assert!(!parent_resolves(&shallow, "HEAD^1"));
    assert!(!parent_resolves(&shallow, "HEAD^2"));

    let control = dispositions(&plan(
        &full,
        "pull_request",
        &origin.second,
        &origin.pr_head,
    )?)?;
    assert!(
        control
            .values()
            .any(|disposition| disposition == "not_affected"),
        "the control clone must discriminate: {control:?}"
    );

    let request = json!({
        "schema": 1,
        "op": "plan-v1",
        "run_key": "r190001-a1",
        "base": origin.second,
        "head": origin.pr_head,
        "event": "pull_request",
        "root": shallow.to_string_lossy(),
    });
    match plan_internal(&request.to_string()) {
        Ok(response) => Err(format!("a shallow merge checkout produced a plan: {response}").into()),
        Err(error) => {
            assert!(
                error.to_string().contains("checkout_head_mismatch"),
                "unexpected planning error: {error}"
            );
            Ok(())
        }
    }
}

#[test]
fn pull_request_plan_without_the_base_commit_runs_every_job() -> Result<(), Box<dyn Error>> {
    let origin = origin()?;
    let full_scratch = tempfile::tempdir()?;
    let full = full_scratch.path().join("work");
    full_clone(&origin, &full, &origin.pr_head)?;
    let shallow_scratch = tempfile::tempdir()?;
    let shallow = shallow_scratch.path().join("work");
    shallow_clone(&origin, &shallow, &origin.pr_head)?;
    assert!(has_commit(&full, &origin.second));
    assert!(!has_commit(&shallow, &origin.second));

    let full_jobs = dispositions(&plan(
        &full,
        "pull_request",
        &origin.second,
        &origin.pr_head,
    )?)?;
    assert!(
        full_jobs
            .values()
            .any(|disposition| disposition == "not_affected"),
        "the control clone must discriminate: {full_jobs:?}"
    );
    assert!(
        full_jobs.values().any(|disposition| disposition == "run"),
        "the control clone must run the changed crate: {full_jobs:?}"
    );

    let shallow_plan = plan(&shallow, "pull_request", &origin.second, &origin.pr_head)?;
    let shallow_jobs = dispositions(&shallow_plan)?;
    assert_eq!(
        full_jobs.keys().collect::<Vec<_>>(),
        shallow_jobs.keys().collect::<Vec<_>>()
    );
    assert!(
        shallow_jobs
            .values()
            .all(|disposition| disposition == "run"),
        "a missing base must select everything: {shallow_jobs:?}"
    );
    assert!(
        warnings(&shallow_plan)
            .iter()
            .any(|warning| warning.starts_with("comparison_unavailable:")),
        "the miss must be reported: {:?}",
        warnings(&shallow_plan)
    );
    Ok(())
}
