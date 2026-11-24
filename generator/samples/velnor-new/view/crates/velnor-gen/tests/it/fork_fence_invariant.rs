// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! A job that a fork pull request can reach must not run on a self-hosted runner.
//! This walks every rendered job instead of naming the renderer paths, so a path that is
//! added later and forgets the fence fails here.

use std::collections::BTreeMap;
use std::error::Error;
use std::fs;

use velnor_gen::expected_files;
use velnor_model::RepoPath;

use crate::call_fork_fence::FENCE;
use crate::golden::{sample_dirs, samples};
use crate::release_fixture::release_view;

type TestResult = Result<(), Box<dyn Error>>;

const RUNS_ON_FENCE: &str = "github.event.pull_request.head.repo.full_name == github.repository";
const EVENTS: [&str; 3] = ["pull_request", "pull_request_target", "workflow_call"];

fn github_hosted(label: &str) -> bool {
    ["ubuntu-", "macos-", "windows-"].iter().any(|family| {
        label.strip_prefix(family).is_some_and(|version| {
            version == "latest" || version.starts_with(|c: char| c.is_ascii_digit())
        })
    })
}

/// Whether a fork pull request that reaches `job` is kept off a self-hosted runner.
fn fenced(job: &serde_json::Value) -> bool {
    let condition = job["if"].as_str().unwrap_or_default();
    if job.get("uses").is_some() {
        return condition.contains(FENCE);
    }
    match &job["runs-on"] {
        serde_json::Value::String(label) => {
            github_hosted(label)
                || label.contains(RUNS_ON_FENCE)
                || condition.contains(FENCE)
                || matrix_hosted(job, label)
        }
        _ => condition.contains(FENCE),
    }
}

/// `runs-on: ${{ matrix.<axis> }}` where every value of the axis is a hosted label.
fn matrix_hosted(job: &serde_json::Value, label: &str) -> bool {
    let Some(axis) = label
        .strip_prefix("${{ matrix.")
        .and_then(|rest| rest.strip_suffix(" }}"))
    else {
        return false;
    };
    let matrix = &job["strategy"]["matrix"];
    let listed = matrix[axis].as_array().into_iter().flatten();
    let included = matrix["include"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.get(axis));
    let values: Vec<&str> = listed.chain(included).filter_map(|v| v.as_str()).collect();
    !values.is_empty() && values.into_iter().all(github_hosted)
}

fn triggers(workflow: &serde_json::Value) -> Vec<String> {
    match workflow
        .get("on")
        .or_else(|| workflow.get(true.to_string()))
    {
        Some(serde_json::Value::String(event)) => vec![event.clone()],
        Some(serde_json::Value::Array(events)) => events
            .iter()
            .filter_map(|event| event.as_str().map(str::to_owned))
            .collect(),
        Some(serde_json::Value::Object(events)) => events.keys().cloned().collect(),
        _ => Vec::new(),
    }
}

/// `file: job` for every job of a workflow a pull request can reach that lacks the fence.
fn unfenced(files: &BTreeMap<String, String>) -> Result<Vec<String>, Box<dyn Error>> {
    let mut found = Vec::new();
    for (path, text) in files {
        if !path.starts_with(".github/workflows/") {
            continue;
        }
        let workflow: serde_json::Value = crate::yaml_tree::document(text)?;
        if !triggers(&workflow)
            .iter()
            .any(|event| EVENTS.contains(&event.as_str()))
        {
            continue;
        }
        for (id, job) in workflow["jobs"].as_object().ok_or("no jobs")? {
            if !fenced(job) {
                found.push(format!("{path}: {id}"));
            }
        }
    }
    Ok(found)
}

fn rendered(view: &velnor_model::RepoView) -> Result<BTreeMap<String, String>, Box<dyn Error>> {
    Ok(expected_files(view)?
        .into_iter()
        .map(|(path, text)| (path.as_str().to_owned(), text))
        .collect())
}

#[test]
fn every_job_of_every_sample_that_a_pull_request_reaches_is_fenced() -> TestResult {
    let mut found = Vec::new();
    for sample in sample_dirs(&samples())? {
        let mut files = BTreeMap::new();
        let expected = sample.join("expected");
        let mut pending = vec![expected.clone()];
        while let Some(directory) = pending.pop() {
            for entry in fs::read_dir(&directory)? {
                let path = entry?.path();
                if path.is_dir() {
                    pending.push(path);
                } else {
                    let relative = path.strip_prefix(&expected)?.to_string_lossy().into_owned();
                    files.insert(relative, fs::read_to_string(&path)?);
                }
            }
        }
        let name = sample.file_name().unwrap_or_default().to_string_lossy();
        found.extend(
            unfenced(&files)?
                .into_iter()
                .map(|job| format!("{name}/{job}")),
        );
    }
    assert!(
        found.is_empty(),
        "jobs a fork pull request reaches without the fence: {found:#?}"
    );
    Ok(())
}

/// A workflow `ci` for pull requests and a callable `shared`, one job of every kind.
const KINDS: &str = r#"
[[workflow]]
file = "leaf"
name = "Leaf"
runner = "velnor-linux-x64"

[[workflow.job]]
id = "build"

[[workflow.job.step]]
name = "Build"
run = "echo ok"

[[workflow]]
file = "shared"
name = "Shared"
runner = "velnor-linux-x64"

[workflow.on]
workflow_call = true

[[workflow.job]]
id = "normal"

[[workflow.job.step]]
name = "Build"
run = "echo ok"

[[workflow.job]]
id = "helper"
helper = true

[[workflow.job.step]]
name = "Build"
run = "echo ok"

[[workflow.job]]
id = "call-local"
uses = "./.github/workflows/hand-written.yml"

[[workflow.job]]
id = "call-pinned"
uses = "tailrocks/velnor-new/.github/workflows/x.yml@0123456789abcdef0123456789abcdef01234567"

[[workflow.job]]
id = "call-declared"
uses = "leaf"

[[workflow]]
file = "ci"
name = "CI"
runner = "velnor-linux-x64"

[workflow.on.pull_request]
types = ["opened"]

[[workflow.job]]
id = "normal"

[[workflow.job.step]]
name = "Build"
run = "echo ok"

[[workflow.job]]
id = "helper"
helper = true

[[workflow.job.step]]
name = "Build"
run = "echo ok"

[[workflow.job]]
id = "call-declared"
uses = "leaf"
"#;

#[test]
fn every_job_kind_that_a_pull_request_reaches_is_fenced_on_a_self_hosted_runner() -> TestResult {
    for shape in [
        "",
        "shape = 'one-job'\n",
        "shape = 'lint-test'\n",
        "shape = 'groups'\n",
    ] {
        let mut view = release_view(&[], None)?;
        view.files.insert(
            RepoPath::new(".velnor/rust-ci.toml")?,
            format!("full_run = \"off\"\nrunner = 'ubuntu-26.04-scale-set'\n{shape}").into_bytes(),
        );
        view.files.insert(
            RepoPath::new(".velnor/workflows.toml")?,
            KINDS.as_bytes().to_vec(),
        );
        let files = rendered(&view)?;
        for path in ["rust-ci", "shared", "ci"] {
            assert!(
                files.contains_key(&format!(".github/workflows/{path}.yml")),
                "{shape}: {path} not rendered: {:?}",
                files.keys()
            );
        }
        let found = unfenced(&files)?;
        assert!(found.is_empty(), "{shape}: {found:#?}");
    }
    Ok(())
}

#[test]
fn the_invariant_flags_a_job_without_the_fence() -> TestResult {
    let workflow = |job: &str| {
        BTreeMap::from([(
            ".github/workflows/x.yml".to_owned(),
            format!("on:\n  pull_request: {{}}\njobs:\n  a:\n{job}"),
        )])
    };
    for bare in [
        "    runs-on: velnor-linux-x64\n",
        "    uses: ./.github/workflows/y.yml\n",
        "    uses: ./.github/workflows/y.yml\n    if: github.ref == 'x'\n",
    ] {
        assert_eq!(unfenced(&workflow(bare))?.len(), 1, "{bare}");
    }
    let fence = format!("${{{{ {FENCE} }}}}");
    let no_violation: Vec<String> = Vec::new();
    let fenced_call = format!("    uses: ./.github/workflows/y.yml\n    if: \"{fence}\"\n");
    assert_eq!(unfenced(&workflow(&fenced_call))?, no_violation);
    assert_eq!(
        unfenced(&workflow("    runs-on: ubuntu-latest\n"))?,
        no_violation
    );
    Ok(())
}
