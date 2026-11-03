// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_actions_contract::WorkflowPolicy;
use velnor_actions_orchestrator::{prepare, render_staged_tree};
use velnor_actions_workflow_renderer::render::WORKFLOW_PATH;

use super::one_job_per_crate::mbx_repo_for;

const BUILD_STEP: &str = "Build ledger CLI";
const PUBLISH_STEP: &str = "Publish ledger CLI outputs";
const LEAN_PROFILE: [(&str, &str); 3] = [
    ("CARGO_PROFILE_DEV_DEBUG", "0"),
    ("CARGO_INCREMENTAL", "0"),
    ("CARGO_PROFILE_DEV_STRIP", "symbols"),
];

struct StepText {
    env: Vec<(String, String)>,
    run: String,
}

fn indent(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

fn unquote(value: &str) -> Result<String, Box<dyn Error>> {
    if value.starts_with('"') {
        Ok(serde_json::from_str(value)?)
    } else {
        Ok(value.to_owned())
    }
}

fn nested(lines: &[&str], from: usize) -> Vec<String> {
    let parent = indent(lines[from]);
    lines[from + 1..]
        .iter()
        .take_while(|line| indent(line) > parent)
        .map(|line| line.trim().to_owned())
        .collect()
}

fn env_entries(
    lines: &[&str],
    env_at: usize,
    step: &str,
) -> Result<Vec<(String, String)>, Box<dyn Error>> {
    let header = lines[env_at].trim();
    let anchor_use = header.strip_prefix("env: *");
    let block_at = match anchor_use {
        Some(alias) => {
            let definition = format!("env: &{alias}");
            lines
                .iter()
                .position(|line| line.trim() == definition)
                .ok_or_else(|| format!("{step}: env alias *{alias} has no definition"))?
        }
        None => env_at,
    };
    nested(lines, block_at)
        .iter()
        .map(|entry| {
            let (key, value) = entry
                .split_once(": ")
                .ok_or_else(|| format!("{step}: env line {entry:?} is not key: value"))?;
            Ok((key.to_owned(), unquote(value)?))
        })
        .collect()
}

fn step_text(text: &str, name: &str) -> Result<StepText, Box<dyn Error>> {
    let lines: Vec<&str> = text.lines().collect();
    let marker = format!("name: {name}");
    let named: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, line)| line.trim().trim_start_matches("- ") == marker)
        .map(|(at, _)| at)
        .collect();
    let [start] = named[..] else {
        return Err(format!("{name}: expected one step, found {}", named.len()).into());
    };
    let floor = indent(lines[start]);
    let body: Vec<&str> = lines[start + 1..]
        .iter()
        .copied()
        .take_while(|line| {
            indent(line) > floor || (indent(line) == floor && !line.trim().starts_with("- "))
        })
        .collect();
    let env_at = body
        .iter()
        .position(|line| line.trim().starts_with("env:"))
        .ok_or_else(|| format!("{name}: step has no env"))?;
    let run = body
        .iter()
        .find_map(|line| line.trim().strip_prefix("run: "))
        .ok_or_else(|| format!("{name}: step has no run"))?;
    Ok(StepText {
        env: env_entries(&body, env_at, name)?,
        run: unquote(run)?,
    })
}

fn render_tree_text(policy: WorkflowPolicy) -> Result<Vec<(String, String)>, Box<dyn Error>> {
    let repo = mbx_repo_for(false, policy)?;
    let tree = render_staged_tree(&prepare(repo.path())?)?;
    Ok(tree
        .files
        .iter()
        .map(|file| (file.path.clone(), file.bytes.clone()))
        .collect())
}

fn occurrences(tree: &[(String, String)], key: &str) -> usize {
    tree.iter()
        .map(|(_, bytes)| bytes.matches(key).count())
        .sum()
}

fn env_values<'a>(env: &'a [(String, String)], key: &str) -> Vec<&'a str> {
    env.iter()
        .filter(|(name, _)| name == key)
        .map(|(_, value)| value.as_str())
        .collect()
}

#[test]
fn ledger_cli_build_uses_the_lean_dev_profile() -> Result<(), Box<dyn Error>> {
    let tree = render_tree_text(WorkflowPolicy::VelnorRepositoryV1)?;
    let workflow = tree
        .iter()
        .find(|(path, _)| path == WORKFLOW_PATH)
        .map(|(_, bytes)| bytes.as_str())
        .ok_or("no ci workflow")?;
    let build = step_text(workflow, BUILD_STEP)?;
    for (key, value) in LEAN_PROFILE {
        assert_eq!(env_values(&build.env, key), [value], "{key}");
    }
    assert_eq!(
        env_values(&build.env, "CARGO_TARGET_DIR"),
        ["${{ runner.temp }}/velnor/ledger-cli-target"]
    );
    assert_eq!(env_values(&build.env, "CARGO_HOME").len(), 1);
    assert!(build.run.contains(" --locked "), "{}", build.run);
    assert!(build.run.contains(" --offline "), "{}", build.run);
    let publish = step_text(workflow, PUBLISH_STEP)?;
    for (key, _) in LEAN_PROFILE {
        assert!(env_values(&publish.env, key).is_empty(), "{key} on publish");
    }
    Ok(())
}

#[test]
fn lean_dev_profile_stays_on_the_ledger_cli_build() -> Result<(), Box<dyn Error>> {
    let velnor = render_tree_text(WorkflowPolicy::VelnorRepositoryV1)?;
    let workflow = velnor
        .iter()
        .find(|(path, _)| path == WORKFLOW_PATH)
        .map(|(_, bytes)| bytes.as_str())
        .ok_or("no ci workflow")?;
    let build = step_text(workflow, BUILD_STEP)?;
    for (key, _) in LEAN_PROFILE {
        assert_eq!(occurrences(&velnor, key), 1, "{key} in velnor tree");
        assert_eq!(env_values(&build.env, key).len(), 1, "{key} on build step");
    }
    let consumer = render_tree_text(WorkflowPolicy::ConsumerV1)?;
    for (key, _) in LEAN_PROFILE {
        assert_eq!(occurrences(&consumer, key), 0, "{key} in consumer tree");
    }
    Ok(())
}
