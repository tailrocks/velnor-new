// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::Value;
use velnor_model::{RepoPath, RepoView};

use crate::events::DENY_MISE;
use crate::facts::TestResult;
use crate::generated_check::CHECK;
use crate::generated_check_source::{FIRST, FROM_SOURCE, locked};

pub(crate) const SAVERS: [&str; 3] = ["rust", "policy", "lint-generated-check"];
const RECORD: &str = "Record anchor";

fn with_files(
    repo: &mut RepoView,
    files: &[(&str, &str)],
) -> Result<(), Box<dyn std::error::Error>> {
    for (path, text) in files {
        let path = RepoPath::new(*path)?;
        repo.tracked_paths.insert(path.clone());
        repo.files.insert(path, text.as_bytes().to_vec());
    }
    Ok(())
}

pub(crate) fn rendered(mise: &str) -> Result<String, Box<dyn std::error::Error>> {
    rendered_as("", None, mise)
}

fn rendered_as(
    shape: &str,
    toolchain: Option<&str>,
    mise: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let config = format!("mbx_shared_store = true\n{shape}{FROM_SOURCE}");
    let mut repo = locked(&config, FIRST)?;
    with_files(&mut repo, &[("deny.toml", ""), ("mise.toml", mise)])?;
    if let Some(toolchain) = toolchain {
        with_files(&mut repo, &[("rust-toolchain.toml", toolchain)])?;
    }
    crate::pipeline::workflow(&repo)
}

pub(crate) fn steps_of(text: &str, id: &str) -> Result<Vec<Value>, Box<dyn std::error::Error>> {
    crate::yaml_tree::document(text)?["jobs"][id]["steps"]
        .as_array()
        .cloned()
        .ok_or_else(|| format!("no job {id}").into())
}

pub(crate) fn position(steps: &[Value], matches: impl Fn(&Value) -> bool) -> Option<usize> {
    steps.iter().position(matches)
}

pub(crate) fn named(name: &'static str) -> impl Fn(&Value) -> bool {
    move |step| step["name"] == name
}

pub(crate) fn runs(step: &Value, word: &str) -> bool {
    step["run"].as_str().is_some_and(|run| run.contains(word))
}

fn without_condition(step: &Value) -> Value {
    let mut step = step.clone();
    if let Some(step) = step.as_object_mut() {
        step.remove("if");
        if step.get("id").is_some_and(|id| id == "prepare") {
            step.remove("run");
        }
    }
    step
}

fn tree(steps: &[Value]) -> Result<&[Value], Box<dyn std::error::Error>> {
    let first = position(steps, |step| {
        step["id"] == "toolslocal" || step["id"] == "tools"
    })
    .ok_or("no tools restore")?;
    let record = position(steps, named(RECORD)).ok_or("no Record anchor")?;
    Ok(&steps[first..=record])
}

const COMPLETE: &str = "[toolchain]\nchannel = \"1.99.1\"\nprofile = \"complete\"\n";
const ANCHOR: [&str; 2] = [
    "needs.rust-plan.outputs.tools_anchor",
    "steps.toolsread.outputs.anchor",
];
const ANCHOR_KEY: [&str; 3] = [
    "needs.rust-plan.outputs.tools_anchor_key",
    "steps.toolsread.outputs.anchor_key",
    "steps.toolsanchor.outputs.cache-matched-key",
];

type Saver = (String, Vec<Value>);

fn savers(text: &str) -> Result<Vec<Saver>, Box<dyn std::error::Error>> {
    let document = crate::yaml_tree::document(text)?;
    let jobs = document["jobs"].as_object().ok_or("no jobs")?;
    let mut found = Vec::new();
    for (id, job) in jobs {
        let steps = job["steps"].as_array().cloned().unwrap_or_default();
        if position(&steps, named(RECORD)).is_some() {
            found.push((id.clone(), steps));
        }
    }
    Ok(found)
}

const TREE_IDS: [&str; 6] = [
    "tools",
    "toolslocal",
    "tools_restored",
    "toolsverify",
    "toolsrecord",
    "toolsanchor",
];
const TREE_USES: [&str; 2] = ["actions/cache/", "jdx/mise-action@"];
const TREE_RUNS: [&str; 4] = [
    ". \"$VELNOR_EXEC\" tools-local ",
    ". \"$VELNOR_EXEC\" tools-verify ",
    "rustup component add ",
    "rustup target add ",
];
const TREE_COMPONENTS: &str = ". \"$VELNOR_EXEC\" prepare --no-locks";

fn belongs_to_the_tree(step: &Value) -> bool {
    step["id"].as_str().is_some_and(|id| TREE_IDS.contains(&id))
        || step["uses"]
            .as_str()
            .is_some_and(|uses| TREE_USES.iter().any(|action| uses.starts_with(action)))
        || step["run"].as_str().is_some_and(|run| {
            run == TREE_COMPONENTS || TREE_RUNS.iter().any(|prefix| run.starts_with(prefix))
        })
}

#[test]
fn only_tool_steps_belong_to_the_tree() {
    let rows = [
        (
            serde_json::json!({"run": "rustup component add clippy"}),
            true,
        ),
        (serde_json::json!({"run": TREE_COMPONENTS}), true),
        (serde_json::json!({"uses": "actions/cache/save@x"}), true),
        (serde_json::json!({"run": "mise run db-up"}), false),
        (
            serde_json::json!({"run": "cargo build", "name": "mise tools"}),
            false,
        ),
        (
            serde_json::json!({"run": ". \"$VELNOR_EXEC\" prepare"}),
            false,
        ),
        (serde_json::json!({"uses": "actions/checkout@x"}), false),
    ];
    for (step, expected) in rows {
        assert_eq!(belongs_to_the_tree(&step), expected, "{step}");
    }
}

#[test]
fn every_tools_saver_builds_the_same_tree() -> TestResult {
    let fixtures = [
        ("one job", "", None),
        ("plan shape", "shape = \"lint-test\"\n", None),
        ("complete profile", "", Some(COMPLETE)),
        (
            "complete profile in the plan shape",
            "shape = \"lint-test\"\n",
            Some(COMPLETE),
        ),
    ];
    for (label, shape, toolchain) in fixtures {
        let text = rendered_as(shape, toolchain, DENY_MISE)?;
        let mut trees = Vec::new();
        for (id, steps) in savers(&text)? {
            let tree = tree(&steps)?;
            let foreign = tree.iter().find(|step| !belongs_to_the_tree(step));
            assert!(foreign.is_none(), "{label} {id}: {foreign:?}");
            let normal = |step: &Value| {
                let step = ANCHOR_KEY
                    .iter()
                    .fold(without_condition(step).to_string(), |text, form| {
                        text.replace(form, "<anchor key>")
                    });
                ANCHOR
                    .iter()
                    .fold(step, |text, form| text.replace(form, "<anchor>"))
            };
            trees.push((id, tree.iter().map(normal).collect::<Vec<_>>()));
        }
        assert!(trees.len() >= SAVERS.len(), "{label}: {trees:?}");
        let (first, expected) = &trees[0];
        for (id, tree) in &trees[1..] {
            assert_eq!(tree, expected, "{label}: {id} differs from {first}");
        }
    }
    Ok(())
}

fn tooling(steps: &[Value]) -> Option<&Value> {
    steps.iter().find(|step| {
        runs(step, "tools-local")
            || runs(step, "tools-verify")
            || step["with"]["key"]
                .as_str()
                .is_some_and(|key| key.starts_with("velnor-tools-"))
    })
}

#[test]
fn a_job_without_tools_renders_no_tools_save() -> TestResult {
    let from_source = steps_of(&rendered(DENY_MISE)?, "lint-generated-check")?;
    assert!(tooling(&from_source).is_some(), "{from_source:?}");
    let mut plain = locked(CHECK, FIRST)?;
    with_files(&mut plain, &[("mise.toml", DENY_MISE)])?;
    let steps = steps_of(&crate::pipeline::workflow(&plain)?, "lint-generated-check")?;
    assert!(tooling(&steps).is_none(), "{steps:?}");
    Ok(())
}
