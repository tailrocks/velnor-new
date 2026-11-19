// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! `execution.overrides` places the source qualification control jobs on the
//! scale set, fenced from fork pull requests.

use std::error::Error;
use std::fs;

use velnor_actions_contract::config::EPHEMERAL_CHECK_ADMISSION_CONDITION;
use velnor_actions_orchestrator::{prepare, render_staged_tree};

use super::source_qualification::{WORKFLOW_PATH, generator_config, generator_repo};

const SCALE_RUNS_ON: &str = "runs-on: [velnor, ubuntu-26.04-scale-set]";
const JOBS: [&str; 6] = [
    "plan",
    "selection",
    "ordinary-harness",
    "keep-harness",
    "generation-checks",
    "source-qualification",
];

fn overrides(ids: &[&str], role: &str) -> String {
    ids.iter()
        .map(|id| format!("\n[execution.overrides.{id}]\nprofile = \"local\"\nrole = \"{role}\"\n"))
        .collect::<Vec<_>>()
        .concat()
}

fn rendered(ids: &[&str], role: &str) -> Result<String, Box<dyn Error>> {
    let repo = generator_repo(true)?;
    let config = format!("{}{}", generator_config(true), overrides(ids, role));
    fs::write(repo.path().join(".velnor/config.toml"), config)?;
    let tree = render_staged_tree(&prepare(repo.path())?)?;
    Ok(tree
        .get(WORKFLOW_PATH)
        .ok_or("source qualification workflow missing")?
        .to_string())
}

fn job_block<'a>(text: &'a str, id: &str) -> Result<&'a str, Box<dyn Error>> {
    let start = text
        .find(&format!("\n  {id}:\n"))
        .ok_or_else(|| format!("job {id} missing"))?;
    let rest = &text[start + 1..];
    let end = rest
        .match_indices("\n  ")
        .find(|(at, _)| rest[at + 3..].chars().next().is_some_and(|c| c != ' '))
        .map_or(rest.len(), |(at, _)| at);
    Ok(&rest[..end])
}

#[test]
fn without_overrides_every_job_stays_hosted_and_unfenced() -> Result<(), Box<dyn Error>> {
    let text = rendered(&[], "control")?;
    assert!(!text.contains(SCALE_RUNS_ON), "{text}");
    assert!(!text.contains("head.repo.full_name"));
    assert_eq!(text.matches("runs-on: ubuntu-26.04").count(), JOBS.len());
    Ok(())
}

#[test]
fn overrides_move_every_job_and_fence_each_from_fork_pull_requests() -> Result<(), Box<dyn Error>> {
    let text = rendered(&JOBS, "control")?;
    for id in JOBS {
        let block = job_block(&text, id)?;
        assert!(block.contains(SCALE_RUNS_ON), "{id}: {block}");
        assert!(
            block.contains("shell: bash -e {0}"),
            "{id} lacks the scale-set shell default"
        );
        assert!(
            block.contains(EPHEMERAL_CHECK_ADMISSION_CONDITION),
            "{id} is not fork fenced"
        );
        assert!(
            block.contains("github.event_name == 'workflow_dispatch')"),
            "{id} drops the manual dispatch"
        );
    }
    assert!(!text.contains("runs-on: ubuntu-26.04"));
    Ok(())
}

#[test]
fn one_override_moves_only_its_job() -> Result<(), Box<dyn Error>> {
    let text = rendered(&["selection"], "control")?;
    assert!(job_block(&text, "selection")?.contains(SCALE_RUNS_ON));
    for id in JOBS.iter().filter(|id| **id != "selection") {
        let block = job_block(&text, id)?;
        assert!(block.contains("runs-on: ubuntu-26.04"), "{id}");
        assert!(!block.contains("head.repo.full_name"), "{id}");
    }
    Ok(())
}

#[test]
fn a_non_control_role_is_rejected() -> Result<(), Box<dyn Error>> {
    let repo = generator_repo(true)?;
    let config = format!(
        "{}{}",
        generator_config(true),
        overrides(&["selection"], "verification")
    );
    fs::write(repo.path().join(".velnor/config.toml"), config)?;
    let error = prepare(repo.path())
        .and_then(|prepared| render_staged_tree(&prepared))
        .err()
        .ok_or("a verification role was accepted for a control job")?;
    assert!(error.to_string().contains("role_not_eligible"), "{error}");
    Ok(())
}
