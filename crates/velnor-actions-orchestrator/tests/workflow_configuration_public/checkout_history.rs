// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::error::Error;
use std::fs;

use velnor_actions_orchestrator::{OrchestratorError, prepare, render_staged_tree};
use velnor_actions_workflow_renderer::render::WORKFLOW_PATH;

use super::workflow_configuration_fixture;

type StepShape = (String, Option<String>);

const PUSH_AWARE_DEPTH: &str = "${{ github.event_name == 'push' && '1' || '0' }}";
const MERGE_PARENTS_DEPTH: &str = "${{ github.event_name == 'push' && '1' || '2' }}";

fn config(default_branch: &str, extra_workflow: &str) -> String {
    format!(
        "schema = 1\n[workflow]\nname = \"CI\"\npolicy = \"consumer-v1\"\ndefault_branch = \"{default_branch}\"\n{extra_workflow}\n[[native_checks]]\nid = \"fixture-native-tree\"\noperation = \"tap_tree\"\n[native_checks.runner]\nlabel = \"ubuntu-26.04\"\nplatform = \"linux_x64\"\nexecutor = \"hosted\"\n\n{}\n{}",
        check("lean", "history = \"shallow_on_push\"\n"),
        check("full", "")
    )
}

fn check(id: &str, history: &str) -> String {
    format!(
        "[[checks]]\nid = \"{id}\"\ntask = \"fixture-check\"\n{history}inputs = [\"Cargo.toml\"]\ntools = []\n[checks.runner]\nlabel = \"ubuntu-26.04\"\nplatform = \"linux_x64\"\nexecutor = \"hosted\"\n"
    )
}

fn rendered_workflow(config: &str) -> Result<String, Box<dyn Error>> {
    let repo = workflow_configuration_fixture::make_repo(config)?;
    fs::write(
        repo.path().join("mise.toml"),
        "[tasks.fixture-check]\nrun = \"true\"\n",
    )?;
    let prepared = prepare(repo.path())?;
    let rendered = render_staged_tree(&prepared)?;
    Ok(rendered
        .get(WORKFLOW_PATH)
        .ok_or("rendered workflow missing")?
        .to_owned())
}

fn job_lines(workflow: &str) -> BTreeMap<String, Vec<&str>> {
    let mut jobs: BTreeMap<String, Vec<&str>> = BTreeMap::new();
    let mut current: Option<String> = None;
    let mut in_jobs = false;
    for line in workflow.lines() {
        if line == "jobs:" {
            in_jobs = true;
            continue;
        }
        if !in_jobs {
            continue;
        }
        let job_header = line
            .strip_prefix("  ")
            .and_then(|rest| rest.strip_suffix(':'))
            .filter(|id| !id.is_empty() && !id.starts_with(' '));
        if let Some(id) = job_header {
            current = Some(id.to_owned());
            jobs.entry(id.to_owned()).or_default();
        } else if let Some(id) = &current {
            jobs.entry(id.clone()).or_default().push(line);
        }
    }
    jobs
}

fn step_blocks<'a>(lines: &[&'a str]) -> Vec<(String, Vec<&'a str>)> {
    let mut blocks: Vec<(String, Vec<&'a str>)> = Vec::new();
    let steps_at = lines.iter().position(|line| *line == "    steps:");
    for line in steps_at.map_or(&[][..], |at| &lines[at + 1..]) {
        if let Some(head) = line.strip_prefix("      - ") {
            let name = head.strip_prefix("name: ").unwrap_or(head);
            blocks.push((name.to_owned(), vec![line]));
        } else if let Some((name, block)) = blocks.last_mut() {
            if block.len() == 1
                && name.starts_with('&')
                && let Some(real) = line.trim().strip_prefix("name: ")
            {
                real.clone_into(name);
            }
            block.push(line);
        }
    }
    blocks
}

fn checkout_depths(workflow: &str) -> BTreeMap<String, Option<String>> {
    let mut depths = BTreeMap::new();
    for (job, lines) in job_lines(workflow) {
        if job.starts_with("tools-producer-") {
            continue;
        }
        let blocks = step_blocks(&lines);
        let Some((_, checkout)) = blocks.iter().find(|(name, _)| name == "Checkout") else {
            continue;
        };
        assert!(
            checkout
                .iter()
                .any(|line| line.trim() == "persist-credentials: \"false\""),
            "{job}: checkout must keep credentials out of the workspace: {checkout:?}"
        );
        let depth = checkout
            .iter()
            .find_map(|line| line.trim().strip_prefix("fetch-depth: "))
            .map(str::to_owned);
        depths.insert(job, depth);
    }
    depths
}

fn step_names(workflow: &str, job: &str) -> Vec<String> {
    let jobs = job_lines(workflow);
    jobs.get(job)
        .map(|lines| {
            step_blocks(lines)
                .into_iter()
                .map(|(name, _)| name)
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn only_the_promised_jobs_take_the_push_aware_checkout_depth() -> Result<(), Box<dyn Error>> {
    let workflow = rendered_workflow(&config("main", ""))?;
    let expected: BTreeMap<String, Option<String>> = [
        ("check-fixture-native-tree", Some("\"0\"")),
        ("check-full", Some("\"0\"")),
        ("check-lean", Some(PUSH_AWARE_DEPTH)),
        ("jactionlint", None),
        ("plan", Some(PUSH_AWARE_DEPTH)),
        ("required", Some(MERGE_PARENTS_DEPTH)),
        ("rust-demo", None),
    ]
    .into_iter()
    .map(|(job, depth)| (job.to_owned(), depth.map(str::to_owned)))
    .collect();
    assert_eq!(checkout_depths(&workflow), expected);
    Ok(())
}

#[test]
fn plan_keeps_its_tools_cache_prelude_with_the_push_aware_checkout() -> Result<(), Box<dyn Error>> {
    let workflow = rendered_workflow(&config("main", ""))?;
    let names = step_names(&workflow, "plan");
    let setup = names
        .iter()
        .position(|name| name == "Setup Mise")
        .ok_or("plan has no Setup Mise")?;
    assert_eq!(
        names.first().map(String::as_str),
        Some("Checkout"),
        "{names:?}"
    );
    assert_eq!(
        names.get(setup + 1..setup + 3),
        Some(&["V2 identity".to_owned(), "Restore Mise tools".to_owned()][..]),
        "the tools cache prelude and restore must follow Setup Mise: {names:?}"
    );
    Ok(())
}

#[test]
fn lean_and_full_checks_differ_only_in_the_checkout_depth() -> Result<(), Box<dyn Error>> {
    let workflow = rendered_workflow(&config("main", ""))?;
    let jobs = job_lines(&workflow);
    let shape = |job: &str| -> Result<Vec<StepShape>, Box<dyn Error>> {
        let lines = jobs.get(job).ok_or("check job missing")?;
        Ok(step_blocks(lines)
            .into_iter()
            .map(|(name, block)| {
                let uses = block
                    .iter()
                    .find_map(|line| line.trim().strip_prefix("uses: "))
                    .map(str::to_owned);
                (name, uses)
            })
            .collect())
    };
    let lean = shape("check-lean")?;
    assert_eq!(lean, shape("check-full")?);
    assert_eq!(
        lean.first().map(|(name, _)| name.as_str()),
        Some("Checkout")
    );
    let depths = checkout_depths(&workflow);
    assert_ne!(depths["check-lean"], depths["check-full"]);
    Ok(())
}

#[test]
fn shallow_check_requires_the_rendered_push_branch_to_be_main() -> Result<(), Box<dyn Error>> {
    let repo = workflow_configuration_fixture::make_repo(&config("testmain", ""))?;
    fs::write(
        repo.path().join("mise.toml"),
        "[tasks.fixture-check]\nrun = \"true\"\n",
    )?;
    let Err(error) = prepare(repo.path()) else {
        return Err("a shallow check on a non-main push branch must not render".into());
    };
    match error {
        OrchestratorError::Config {
            key_path, problem, ..
        } => {
            assert_eq!(key_path, "checks.lean.history");
            assert_eq!(problem, "shallow_on_push_requires_push_branch_main");
        }
        other => return Err(format!("unexpected error: {other}").into()),
    }

    let workflow = rendered_workflow(&config(
        "main",
        "verification_branch = \"sea/integration\"\n",
    ))?;
    let push = workflow
        .split("  push:\n")
        .nth(1)
        .ok_or("rendered push trigger missing")?;
    assert!(
        push.starts_with("    branches:\n      - main\n  merge_group:"),
        "{push}"
    );
    Ok(())
}
