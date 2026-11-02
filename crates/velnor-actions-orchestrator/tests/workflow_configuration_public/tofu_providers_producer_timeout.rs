// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;

use tempfile::TempDir;
use velnor_actions_contract::workflow::ProducerKind;
use velnor_actions_contract::{Job, WorkflowPolicy};
use velnor_actions_orchestrator::{finalized_jobs, prepare, render_staged_tree};
use velnor_actions_workflow_renderer::render::WORKFLOW_PATH;

use super::maintenance_features::owner_feature_repo;
use super::support::rendered_job_block;
use super::tofu_providers_producers::{Jobs, tofu_crate_jobs, tofu_repo_with};
use super::tools_producers::is_tools_producer;

const JOB_TIMEOUT_PREFIX: &str = "    timeout-minutes: ";
const SECONDS_PER_MINUTE: u32 = 60;

fn parse_all<'a>(values: impl Iterator<Item = &'a str>) -> Result<Vec<u32>, Box<dyn Error>> {
    values
        .map(|value| Ok(value.trim_matches(['"', '\'']).parse()?))
        .collect()
}

fn job_timeouts(block: &str) -> Result<Vec<u32>, Box<dyn Error>> {
    parse_all(
        block
            .lines()
            .filter_map(|line| line.strip_prefix(JOB_TIMEOUT_PREFIX)),
    )
}

fn env_numbers(block: &str, name: &str) -> Result<Vec<u32>, Box<dyn Error>> {
    let prefix = format!("{name}: ");
    parse_all(
        block
            .lines()
            .filter_map(|line| line.trim().strip_prefix(&prefix)),
    )
}

fn single(values: &[u32], what: &str, id: &str) -> Result<u32, Box<dyn Error>> {
    match values {
        [value] => Ok(*value),
        _ => Err(format!("{id}: expected one {what}, found {values:?}").into()),
    }
}

fn admitted_slack(id: &str, block: &str) -> Result<(u32, u32), Box<dyn Error>> {
    let minutes = single(&job_timeouts(block)?, "job timeout", id)?;
    let job_seconds = env_numbers(block, "VELNOR_CACHE_JOB_SECONDS")?;
    assert!(!job_seconds.is_empty(), "{id}: no admission clock");
    let required = single(
        &env_numbers(block, "VELNOR_CACHE_REQUIRED_SECONDS")?,
        "required seconds",
        id,
    )?;
    for seconds in job_seconds {
        assert_eq!(seconds, minutes * SECONDS_PER_MINUTE, "{id}");
        assert!(
            seconds >= required,
            "{id}: {seconds}s job cannot admit a {required}s save"
        );
    }
    Ok((minutes, minutes * SECONDS_PER_MINUTE - required))
}

fn assert_tofu_producer_budgets(
    blocks: &[(String, String)],
    crate_timeout: u32,
) -> Result<(), Box<dyn Error>> {
    assert!(!blocks.is_empty(), "no producers rendered");
    for (id, block) in blocks {
        let (minutes, slack) = admitted_slack(id, block)?;
        if crate_timeout == 1 {
            assert!(
                slack < SECONDS_PER_MINUTE,
                "{id}: inflated to {minutes} min"
            );
        } else {
            assert_eq!(minutes, crate_timeout, "{id}");
        }
    }
    Ok(())
}

fn rendered(repo: &TempDir) -> Result<(String, Jobs), Box<dyn Error>> {
    let prepared = prepare(repo.path())?;
    let tree = render_staged_tree(&prepared)?;
    let ci = tree.get(WORKFLOW_PATH).ok_or("CI workflow is missing")?;
    Ok((ci.to_owned(), finalized_jobs(&prepared)?))
}

fn producer_blocks(
    ci: &str,
    jobs: &Jobs,
    is_producer: fn(&Job) -> bool,
) -> Result<Vec<(String, String)>, Box<dyn Error>> {
    jobs.iter()
        .filter(|(_, job)| is_producer(job))
        .map(|(id, _)| Ok((id.clone(), rendered_job_block(ci, id)?.to_owned())))
        .collect()
}

fn is_tofu_producer(job: &Job) -> bool {
    job.producer_kind() == Some(ProducerKind::TofuProviders)
}

#[test]
fn tofu_producer_timeout_admits_its_save_and_never_shrinks_the_crate_timeout()
-> Result<(), Box<dyn Error>> {
    for crate_timeout in [1_u16, 15] {
        let repo = tofu_repo_with(&["infra", "infra2", "infra3"], Some(crate_timeout))?;
        let (ci, jobs) = rendered(&repo)?;
        let blocks = producer_blocks(&ci, &jobs, is_tofu_producer)?;
        assert_tofu_producer_budgets(&blocks, u32::from(crate_timeout))?;
        let crates = tofu_crate_jobs(&jobs);
        assert!(!crates.is_empty());
        for (id, _) in crates {
            let block = rendered_job_block(&ci, id)?;
            assert!(
                block.contains(&format!("{JOB_TIMEOUT_PREFIX}{crate_timeout}\n")),
                "{id}: {block}"
            );
        }
    }
    Ok(())
}

fn tools_repo(crate_timeout: u16) -> Result<TempDir, Box<dyn Error>> {
    let repo = owner_feature_repo(WorkflowPolicy::ConsumerV1, false, "default", "default")?;
    let path = repo.path().join(".velnor/config.toml");
    let config = fs::read_to_string(&path)?;
    let marker = "default_branch = \"testmain\"\n";
    assert!(config.contains(marker));
    fs::write(
        &path,
        config.replacen(
            marker,
            &format!("{marker}crate_timeout_minutes = {crate_timeout}\n"),
            1,
        ),
    )?;
    Ok(repo)
}

#[test]
fn tools_producer_donated_a_one_minute_crate_timeout_still_admits_its_save()
-> Result<(), Box<dyn Error>> {
    let (ci, jobs) = rendered(&tools_repo(1)?)?;
    let blocks = producer_blocks(&ci, &jobs, is_tools_producer)?;
    assert!(!blocks.is_empty(), "no producers rendered");
    let mut lifted = 0;
    for (id, block) in &blocks {
        let (_, slack) = admitted_slack(id, block)?;
        lifted += usize::from(slack < SECONDS_PER_MINUTE);
    }
    assert_ne!(lifted, 0, "no producer took the one-minute crate donor");
    Ok(())
}
