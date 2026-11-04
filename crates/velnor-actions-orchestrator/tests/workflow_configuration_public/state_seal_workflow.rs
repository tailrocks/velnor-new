// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::error::Error;
use std::fs;
use std::path::Path;
use std::process::Command;

use serde_json::{Value, json};
use velnor_actions_contract::workflow::ProducerKind;
use velnor_actions_contract::workflow::state_seal::STATE_SEAL_WORKFLOW_PATH;
use velnor_actions_orchestrator::{finalized_jobs, prepare, render_staged_tree};
use velnor_actions_workflow_renderer::render::WORKFLOW_PATH;

use super::seed_off::seed_repo;
use super::state_roots::declared_input_repo;
use super::workflow_configuration_fixture::git;

const DEFAULT_BRANCH: &str = "testmain";
const REQUIRED_NEEDS: [&str; 5] = ["jactionlint", "plan", "rust-a-b", "rust-c", "rust-demo"];

struct Pair {
    ci: Value,
    ci_text: String,
    sealed: Value,
    sealed_text: String,
    moved: BTreeSet<String>,
}

fn pair() -> Result<Pair, Box<dyn Error>> {
    let repo = seed_repo("")?;
    let prepared = prepare(repo.path())?;
    let combined = finalized_jobs(&prepared)?;
    let tree = render_staged_tree(&prepared)?;
    let text = |path: &str| -> Result<String, Box<dyn Error>> {
        Ok(tree
            .get(path)
            .ok_or_else(|| format!("{path} was not rendered"))?
            .to_owned())
    };
    let ci_text = text(WORKFLOW_PATH)?;
    let sealed_text = text(STATE_SEAL_WORKFLOW_PATH)?;
    let moved = combined
        .iter()
        .filter(|(_, job)| {
            job.producer_kind()
                .is_some_and(|kind| kind != ProducerKind::TofuProviders)
        })
        .map(|(id, _)| id.clone())
        .collect();
    Ok(Pair {
        ci: serde_saphyr::from_str(&ci_text)?,
        ci_text,
        sealed: serde_saphyr::from_str(&sealed_text)?,
        sealed_text,
        moved,
    })
}

fn jobs(workflow: &Value) -> Result<&serde_json::Map<String, Value>, Box<dyn Error>> {
    workflow["jobs"]
        .as_object()
        .ok_or_else(|| "workflow has no jobs".into())
}

fn needs_of(job: &Value) -> Vec<String> {
    job["needs"]
        .as_array()
        .map(|needs| {
            needs
                .iter()
                .filter_map(|need| need.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

fn closure(jobs: &serde_json::Map<String, Value>, roots: &[&str]) -> BTreeSet<String> {
    let mut seen = BTreeSet::new();
    let mut stack: Vec<String> = roots.iter().map(|id| (*id).to_owned()).collect();
    while let Some(id) = stack.pop() {
        if seen.insert(id.clone())
            && let Some(job) = jobs.get(&id)
        {
            stack.extend(needs_of(job));
        }
    }
    seen
}

#[test]
fn the_ci_run_ends_when_required_ends() -> Result<(), Box<dyn Error>> {
    let pair = pair()?;
    let ci_jobs = jobs(&pair.ci)?;
    let reached = closure(ci_jobs, &["required", "required-gate"]);
    let stray: Vec<&String> = ci_jobs.keys().filter(|id| !reached.contains(*id)).collect();
    assert!(stray.is_empty(), "jobs outside Required: {stray:?}");
    for workflow in [&pair.ci, &pair.sealed] {
        let all = jobs(workflow)?;
        for (id, job) in all {
            for need in needs_of(job) {
                assert!(all.contains_key(&need), "{id} needs a missing job {need}");
            }
        }
    }
    let mut required = needs_of(&ci_jobs["required"]);
    required.sort();
    assert_eq!(required, REQUIRED_NEEDS);
    for id in &pair.moved {
        assert!(!ci_jobs.contains_key(id), "{id} is still in ci.yml");
    }
    Ok(())
}

#[test]
fn state_seal_workflow_is_push_only_and_owns_every_writer() -> Result<(), Box<dyn Error>> {
    let pair = pair()?;
    let sealed = &pair.sealed;
    assert_ne!(DEFAULT_BRANCH, "main");
    assert_eq!(
        sealed["on"],
        json!({"push": {"branches": [DEFAULT_BRANCH]}})
    );
    assert_eq!(sealed["permissions"], json!({"contents": "read"}));
    assert_eq!(
        sealed["concurrency"],
        json!({"group": "velnor-${{ github.workflow }}-${{ github.ref }}", "cancel-in-progress": false})
    );
    let all = jobs(sealed)?;
    let ids: BTreeSet<String> = all.keys().cloned().collect();
    let mut expected = pair.moved.clone();
    expected.insert("plan".to_owned());
    assert_eq!(ids, expected);
    assert_eq!(needs_of(&all["plan"]), Vec::<String>::new());
    for (id, job) in all {
        if id == "plan" {
            continue;
        }
        let needs = needs_of(job);
        if id.starts_with("state-producer-") || id == "sources-producer" {
            assert_eq!(needs, ["plan"], "{id}");
        }
        if let Some(root) = id.strip_prefix("state-seal-") {
            assert_eq!(needs.first().map(String::as_str), Some("plan"), "{id}");
            assert_eq!(
                needs.get(1).map(String::as_str),
                Some(format!("state-producer-{root}").as_str()),
                "{id}"
            );
            assert_eq!(
                job["permissions"],
                json!({
                    "actions": "write",
                    "id-token": "write",
                    "attestations": "write",
                    "artifact-metadata": "write",
                    "contents": "read",
                }),
                "{id}"
            );
        } else {
            let permissions = &job["permissions"];
            assert!(
                permissions.is_null() || permissions.get("actions") != Some(&json!("write")),
                "{id}"
            );
        }
    }
    let seal_ids: Vec<&String> = all
        .keys()
        .filter(|id| id.starts_with("state-seal-"))
        .collect();
    assert_eq!(seal_ids.len(), 2);
    let ci_ids: BTreeSet<&String> = jobs(&pair.ci)?.keys().collect();
    for id in &pair.moved {
        assert!(!ci_ids.contains(id), "{id} is written by both workflows");
    }
    Ok(())
}

#[test]
fn each_producer_runs_on_the_runner_class_of_its_crate_jobs() -> Result<(), Box<dyn Error>> {
    let pair = pair()?;
    let ci_jobs = jobs(&pair.ci)?;
    let crate_runners: BTreeSet<&str> = ci_jobs
        .iter()
        .filter(|(id, _)| id.starts_with("rust-"))
        .filter_map(|(_, job)| job["runs-on"].as_str())
        .collect();
    assert_eq!(crate_runners.len(), 1, "{crate_runners:?}");
    let mut checked = BTreeSet::new();
    for (id, job) in jobs(&pair.sealed)? {
        let writer = id.starts_with("state-producer-")
            || id == "sources-producer"
            || id.starts_with("tools-producer-");
        if writer {
            assert_eq!(
                job["runs-on"].as_str(),
                crate_runners.iter().next().copied(),
                "{id}"
            );
            checked.insert(id.split_inclusive('-').take(2).collect::<String>());
        }
    }
    for family in ["state-producer-", "sources-producer", "tools-producer-"] {
        assert!(
            checked.iter().any(|id| id.starts_with(family)),
            "{family} was not checked: {checked:?}"
        );
    }
    Ok(())
}

fn plan_outputs(workflow: &Value) -> BTreeSet<String> {
    workflow["jobs"]["plan"]["outputs"]
        .as_object()
        .map(|outputs| outputs.keys().cloned().collect())
        .unwrap_or_default()
}

fn referenced_plan_outputs(text: &str) -> BTreeSet<String> {
    let marker = "needs.plan.outputs.";
    let mut names = BTreeSet::new();
    let mut rest = text;
    while let Some(at) = rest.find(marker) {
        rest = &rest[at + marker.len()..];
        let end = rest
            .find(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_'))
            .unwrap_or(rest.len());
        names.insert(rest[..end].to_owned());
    }
    names
}

#[test]
fn every_plan_output_a_workflow_reads_is_declared_by_its_own_plan() -> Result<(), Box<dyn Error>> {
    let pair = pair()?;
    for (label, workflow, text) in [
        ("ci.yml", &pair.ci, &pair.ci_text),
        ("state-seal.yml", &pair.sealed, &pair.sealed_text),
    ] {
        let declared = plan_outputs(workflow);
        let missing: Vec<String> = referenced_plan_outputs(text)
            .into_iter()
            .filter(|name| !declared.contains(name))
            .collect();
        assert!(missing.is_empty(), "{label} reads undeclared {missing:?}");
    }
    Ok(())
}

fn cache_keys_outputs(
    workflow: &Value,
    repo: &Path,
    runner_temp: &Path,
) -> Result<String, Box<dyn Error>> {
    let steps = workflow["jobs"]["plan"]["steps"]
        .as_array()
        .ok_or("plan has no steps")?;
    let step = steps
        .iter()
        .find(|step| step["id"] == "cache-keys")
        .ok_or("plan lost its cache keys step")?;
    let script = step["run"].as_str().ok_or("cache keys has no script")?;
    let output = runner_temp.join("plan_output");
    fs::write(&output, "")?;
    let mut command = Command::new("sh");
    command
        .args(["-c", script])
        .env("RUNNER_TEMP", runner_temp)
        .env("GITHUB_WORKSPACE", repo)
        .env("GITHUB_OUTPUT", &output)
        .env("ImageOS", "ubuntu24");
    for (name, value) in step["env"].as_object().ok_or("cache keys has no env")? {
        let value = value.as_str().ok_or("env value is not text")?;
        let resolved = if name == "VELNOR_DEFAULT_REF" {
            "refs/heads/testmain"
        } else if value.contains("${{") {
            "1.2.3"
        } else {
            value
        };
        command.env(name, resolved);
    }
    let run = command.output()?;
    if !run.status.success() {
        return Err(String::from_utf8_lossy(&run.stderr).into_owned().into());
    }
    Ok(fs::read_to_string(&output)?)
}

#[test]
fn both_plans_compute_byte_identical_cache_keys() -> Result<(), Box<dyn Error>> {
    let repo = declared_input_repo()?;
    let prepared = prepare(repo.path())?;
    let tree = render_staged_tree(&prepared)?;
    let parse = |path: &str| -> Result<Value, Box<dyn Error>> {
        Ok(serde_saphyr::from_str(
            tree.get(path).ok_or_else(|| format!("{path} missing"))?,
        )?)
    };
    let ci = parse(WORKFLOW_PATH)?;
    let sealed = parse(STATE_SEAL_WORKFLOW_PATH)?;
    git(&["add", "-A"], repo.path())?;
    let temp = tempfile::tempdir()?;
    let from_ci = cache_keys_outputs(&ci, repo.path(), temp.path())?;
    let from_sealed = cache_keys_outputs(&sealed, repo.path(), temp.path())?;
    assert!(
        from_ci.contains("state_crates_nested_compat=s256-"),
        "{from_ci}"
    );
    assert!(
        from_ci.contains("state_crates_nested_snapshot=s256-"),
        "{from_ci}"
    );
    assert_eq!(from_ci, from_sealed);
    let step = |workflow: &Value| {
        workflow["jobs"]["plan"]["steps"]
            .as_array()
            .and_then(|steps| steps.iter().find(|step| step["id"] == "cache-keys"))
            .cloned()
    };
    assert_eq!(step(&ci), step(&sealed));
    let select = |workflow: &Value| {
        workflow["jobs"]["plan"]["steps"]
            .as_array()
            .and_then(|steps| steps.iter().find(|step| step["id"] == "state-select"))
            .cloned()
    };
    assert_eq!(select(&ci), select(&sealed));
    Ok(())
}
