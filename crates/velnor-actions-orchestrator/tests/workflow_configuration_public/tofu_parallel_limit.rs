// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;

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
    for (id, job) in found {
        assert_producer_shape(id, job, &jobs)?;
    }
    Ok(())
}
