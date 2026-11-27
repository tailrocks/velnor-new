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
    let config = format!("mbx_shared_store = true\n{FROM_SOURCE}");
    let mut repo = locked(&config, FIRST)?;
    with_files(&mut repo, &[("deny.toml", ""), ("mise.toml", mise)])?;
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

#[test]
fn every_tools_saver_builds_the_same_tree() -> TestResult {
    let text = rendered(DENY_MISE)?;
    let mut trees = Vec::new();
    for id in SAVERS {
        let steps = steps_of(&text, id)?;
        let tree = tree(&steps)?;
        let project = tree.iter().find(|step| runs(step, "cargo "));
        assert!(project.is_none(), "{id}: {project:?}");
        trees.push((id, tree.iter().map(without_condition).collect::<Vec<_>>()));
    }
    let (first, expected) = &trees[0];
    for (id, tree) in &trees[1..] {
        assert_eq!(tree, expected, "{id} differs from {first}");
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
