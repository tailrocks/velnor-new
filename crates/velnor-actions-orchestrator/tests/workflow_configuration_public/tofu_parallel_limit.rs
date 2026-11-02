// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;

use velnor_actions_orchestrator::{prepare, render_staged_tree};
use velnor_actions_workflow_renderer::render::WORKFLOW_PATH;

use super::support::rendered_job_block;
use super::tofu_providers_producer_condition::survival_condition;
use super::tofu_providers_producers::{
    Jobs, TOFU_LIMIT, assert_producer_shape, finalized, producers, tofu_crate_jobs, tofu_repo,
};

fn peak_concurrency(lane: &BTreeSet<&str>, jobs: &Jobs) -> Result<usize, Box<dyn Error>> {
    let mut wave: BTreeMap<&str, usize> = BTreeMap::new();
    while wave.len() < lane.len() {
        let before = wave.len();
        for id in lane {
            if wave.contains_key(id) {
                continue;
            }
            let job = jobs.get(*id).ok_or("lane job missing")?;
            let upstream: Vec<Option<&usize>> = job
                .needs
                .iter()
                .filter(|need| lane.contains(need.as_str()))
                .map(|need| wave.get(need.as_str()))
                .collect();
            if upstream.iter().all(Option::is_some) {
                let start = upstream
                    .into_iter()
                    .flatten()
                    .max()
                    .map_or(0, |last| last + 1);
                wave.insert(id, start);
            }
        }
        if wave.len() == before {
            return Err("tofu lane has a dependency cycle".into());
        }
    }
    let mut running: BTreeMap<usize, usize> = BTreeMap::new();
    for start in wave.values() {
        *running.entry(*start).or_default() += 1;
    }
    Ok(running.into_values().max().unwrap_or_default())
}

#[test]
fn tofu_crate_jobs_and_producers_share_the_parallel_job_limit() -> Result<(), Box<dyn Error>> {
    let jobs = finalized(&tofu_repo(&["infra", "infra2", "infra3"])?)?;
    let crates = tofu_crate_jobs(&jobs);
    let found = producers(&jobs);
    assert_eq!((crates.len(), found.len()), (3, 3));
    let lane: BTreeSet<&str> = crates
        .iter()
        .chain(&found)
        .map(|(id, _)| id.as_str())
        .collect();
    let peak = peak_concurrency(&lane, &jobs)?;
    assert!(peak <= TOFU_LIMIT, "{peak} tofu jobs start together");
    for (id, job) in &crates {
        let queued = job.needs.iter().any(|need| lane.contains(need.as_str()));
        let survives_skips = job
            .condition
            .as_deref()
            .is_some_and(|condition| condition.starts_with("always()"));
        assert!(!queued || survives_skips, "{id}: {:?}", job.condition);
    }
    let mut queued_producers = 0;
    for (id, job) in &found {
        if job.needs.iter().any(|need| lane.contains(need.as_str())) {
            queued_producers += 1;
            assert_eq!(
                job.condition.as_deref(),
                Some(survival_condition().as_str()),
                "{id} does not survive a failed predecessor"
            );
        }
        assert_producer_shape(id, job, &jobs)?;
    }
    assert_ne!(queued_producers, 0);
    Ok(())
}

#[test]
fn rendered_queued_producers_survive_a_failed_predecessor_and_stay_push_gated()
-> Result<(), Box<dyn Error>> {
    let repo = tofu_repo(&["infra", "infra2", "infra3"])?;
    let tree = render_staged_tree(&prepare(repo.path())?)?;
    let ci = tree.get(WORKFLOW_PATH).ok_or("CI workflow is missing")?;
    let jobs = finalized(&repo)?;
    let lane: BTreeSet<&str> = tofu_crate_jobs(&jobs)
        .iter()
        .chain(&producers(&jobs))
        .map(|(id, _)| id.as_str())
        .collect();
    let mut queued_producers = 0;
    for (id, job) in producers(&jobs) {
        if !job.needs.iter().any(|need| lane.contains(need.as_str())) {
            continue;
        }
        queued_producers += 1;
        let block = rendered_job_block(ci, id)?;
        let rendered_if = format!("    if: {}\n", survival_condition());
        assert_eq!(block.matches(&rendered_if).count(), 1, "{id}: {block}");
        assert!(block.contains("github.ref_protected == true"), "{id}");
    }
    assert_ne!(queued_producers, 0);
    Ok(())
}
