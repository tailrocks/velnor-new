// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

pub const DEFAULT_UNKNOWN_WEIGHT: u64 = 1;

#[must_use]
pub fn assign_lanes(task_ids: &[String]) -> Vec<(String, u32)> {
    let mut sorted = task_ids.to_vec();
    sorted.sort();
    sorted
        .into_iter()
        .enumerate()
        .map(|(index, id)| (id, u32::try_from(index).unwrap_or(u32::MAX)))
        .collect()
}

#[must_use]
pub fn resource_exclusions(assignments: &[(&str, u32)]) -> Vec<(String, String)> {
    let mut by_lane: BTreeMap<u32, Vec<&str>> = BTreeMap::new();
    for (task, lane) in assignments {
        by_lane.entry(*lane).or_default().push(task);
    }
    let mut pairs = BTreeSet::new();
    for tasks in by_lane.values() {
        let mut sorted = tasks.clone();
        sorted.sort_unstable();
        for (index, left) in sorted.iter().enumerate() {
            for right in sorted.iter().skip(index + 1) {
                pairs.insert(((*left).to_owned(), (*right).to_owned()));
            }
        }
    }
    pairs.into_iter().collect()
}

#[must_use]
pub fn effective_weight(duration_ms: Option<u64>) -> u64 {
    duration_ms.unwrap_or(DEFAULT_UNKNOWN_WEIGHT).max(1)
}

#[must_use]
pub fn partition_count(test_count: usize, configured_shards: u32) -> u32 {
    let configured = configured_shards.max(1);
    let count = u32::try_from(test_count).unwrap_or(u32::MAX);
    if count <= configured { 1 } else { configured }
}

#[must_use]
pub fn distribute_by_weight(weights: &[u64], shards: u32) -> Vec<Vec<usize>> {
    let shards = usize::try_from(shards.max(1)).unwrap_or(1);
    let mut assignment: Vec<Vec<usize>> = vec![Vec::new(); shards];
    let mut totals = vec![0u64; shards];
    let mut order: Vec<usize> = (0..weights.len()).collect();
    order.sort_by_key(|index| u64::MAX - weights[*index]);
    for index in order {
        let mut best = 0;
        for candidate in 1..shards {
            if totals[candidate] < totals[best] {
                best = candidate;
            }
        }
        totals[best] = totals[best].saturating_add(weights[index]);
        assignment[best].push(index);
    }
    for shard in &mut assignment {
        shard.sort_unstable();
    }
    assignment
}

#[must_use]
pub fn fanout_worthwhile(measured_saving_ms: u64, setup_ms: u64, transfer_ms: u64) -> bool {
    measured_saving_ms > setup_ms.saturating_add(transfer_ms)
}

#[must_use]
pub fn sequential_reference(task_ids: &[String]) -> Vec<String> {
    let mut reference = task_ids.to_vec();
    reference.sort();
    reference
}

#[must_use]
#[expect(
    clippy::cast_precision_loss,
    reason = "millisecond spans never approach 2^53; ratio only"
)]
pub fn overlap_ratio(spans: &[(u64, u64)]) -> f64 {
    let total: u64 = spans
        .iter()
        .map(|(start, end)| end.saturating_sub(*start))
        .sum();
    if total == 0 {
        return 0.0;
    }
    let mut sorted = spans.to_vec();
    sorted.sort_unstable();
    let mut union = 0u64;
    let mut cursor = 0u64;
    for (start, end) in sorted {
        let begin = start.max(cursor);
        if end > begin {
            union += end - begin;
            cursor = end;
        }
    }
    1.0 - (union as f64 / total as f64)
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TaskTiming {
    pub queue_ms: u64,
    pub runner_ms: u64,
    pub task_ms: u64,
    pub cache_ms: u64,
    pub prep_ms: u64,
    pub download_ms: u64,
    pub compiler_ms: u64,
    pub mbx_ms: u64,
    pub test_ms: u64,
    pub lock_wait_ms: u64,
}

impl TaskTiming {
    #[must_use]
    pub fn accounted_total(&self) -> u64 {
        self.queue_ms
            .saturating_add(self.runner_ms)
            .saturating_add(self.task_ms)
            .saturating_add(self.cache_ms)
            .saturating_add(self.prep_ms)
            .saturating_add(self.download_ms)
            .saturating_add(self.compiler_ms)
            .saturating_add(self.mbx_ms)
            .saturating_add(self.test_ms)
            .saturating_add(self.lock_wait_ms)
    }

    #[must_use]
    pub fn exclusive_ms(wall_ms: u64, children_union_ms: u64) -> u64 {
        wall_ms.saturating_sub(children_union_ms)
    }
}

#[must_use]
pub fn contract_timing(timing: &TaskTiming) -> velnor_actions_contract::TaskTiming {
    velnor_actions_contract::TaskTiming {
        queue_ms: timing.queue_ms,
        runner_ms: timing.runner_ms,
        task_ms: timing.task_ms,
        cache_ms: timing.cache_ms,
        prep_ms: timing.prep_ms,
        download_ms: timing.download_ms,
        compiler_ms: timing.compiler_ms,
        mbx_ms: timing.mbx_ms,
        test_ms: timing.test_ms,
        lock_wait_ms: timing.lock_wait_ms,
    }
}

#[must_use]
pub fn measured_timing(duration_ms: Option<u64>) -> Option<velnor_actions_contract::TaskTiming> {
    duration_ms.map(|elapsed| {
        contract_timing(&TaskTiming {
            task_ms: elapsed,
            ..TaskTiming::default()
        })
    })
}

#[must_use]
pub fn aggregate_timings(timings: &[TaskTiming]) -> TaskTiming {
    let mut total = TaskTiming::default();
    for timing in timings {
        total.queue_ms = total.queue_ms.saturating_add(timing.queue_ms);
        total.runner_ms = total.runner_ms.saturating_add(timing.runner_ms);
        total.task_ms = total.task_ms.saturating_add(timing.task_ms);
        total.cache_ms = total.cache_ms.saturating_add(timing.cache_ms);
        total.prep_ms = total.prep_ms.saturating_add(timing.prep_ms);
        total.download_ms = total.download_ms.saturating_add(timing.download_ms);
        total.compiler_ms = total.compiler_ms.saturating_add(timing.compiler_ms);
        total.mbx_ms = total.mbx_ms.saturating_add(timing.mbx_ms);
        total.test_ms = total.test_ms.saturating_add(timing.test_ms);
        total.lock_wait_ms = total.lock_wait_ms.saturating_add(timing.lock_wait_ms);
    }
    total
}

#[must_use]
pub fn cache_ownership_table() -> Vec<(&'static str, &'static str)> {
    vec![
        ("~/.local/share/mise", "catalog/tools"),
        ("$CARGO_HOME/registry", "velnor/sources"),
        ("$CARGO_HOME/git", "velnor/sources"),
        ("mr-boxington-action/objects", "mr-boxington/MBX"),
        ("$RUNNER_TEMP/velnor/target/", "job/target"),
        ("$MISE_TASK_CACHE_DIR/task-artifacts/v2", "mise/task-result"),
    ]
}
