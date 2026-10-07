//! Scheduling policy: lanes, partitions, fan-out, timing (par §7-§9).
//!
//! Pure planner policy with no IO: deterministic lane assignment, test
//! partitioning weights, the measurement fan-out gate, the sequential
//! reference, per-task timing breakdowns, overlap accounting, and the
//! single-owner cache-path table (cache §2).

use std::collections::{BTreeMap, BTreeSet};

/// Weight for a test with unknown duration (par §8: default, selected).
pub const DEFAULT_UNKNOWN_WEIGHT: u64 = 1;

/// Assign one deterministic lane per task ID (par §7).
///
/// Lanes follow sorted task-ID order, so concurrent Cargo writers never
/// share a target dir and the assignment is stable under reordering.
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

/// Resource-exclusion pairs for assignments sharing a lane (par §7).
///
/// Every pair of tasks on one lane conflicts on the lane target dir and
/// must serialize; pairs sort ascending and never repeat.
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

/// Effective scheduling weight: measured duration or the default (par §8).
#[must_use]
pub fn effective_weight(duration_ms: Option<u64>) -> u64 {
    duration_ms.unwrap_or(DEFAULT_UNKNOWN_WEIGHT).max(1)
}

/// Step count for a suite: small suites collapse to one step (par §8).
///
/// Never more steps than tests; a one-test suite always runs unsharded.
#[must_use]
pub fn partition_count(test_count: usize, configured_shards: u32) -> u32 {
    let configured = configured_shards.max(1);
    let count = u32::try_from(test_count).unwrap_or(u32::MAX);
    if count <= configured { 1 } else { configured }
}

/// Greedy weight-balanced shard assignment covering every test (par §8).
///
/// Durations are weights only: every index lands in exactly one shard and
/// none is ever omitted.
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

/// Fan-out only when measured savings exceed setup plus transfer (par §8).
#[must_use]
pub fn fanout_worthwhile(measured_saving_ms: u64, setup_ms: u64, transfer_ms: u64) -> bool {
    measured_saving_ms > setup_ms.saturating_add(transfer_ms)
}

/// Sequential-reference obligation order: sorted task IDs (par §10).
#[must_use]
pub fn sequential_reference(task_ids: &[String]) -> Vec<String> {
    let mut reference = task_ids.to_vec();
    reference.sort();
    reference
}

/// Overlap ratio over wall-clock spans (par §9).
///
/// Descriptive only: overlapped time over total time, never a speedup
/// claim. Empty input yields zero.
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

/// Per-task timing breakdown with separately measured slots (par §9).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TaskTiming {
    /// Queue wait before dispatch.
    pub queue_ms: u64,
    /// Runner provisioning.
    pub runner_ms: u64,
    /// Task-body wall time.
    pub task_ms: u64,
    /// Cache restore/save handling.
    pub cache_ms: u64,
    /// Preparation before the payload.
    pub prep_ms: u64,
    /// Artifact downloads.
    pub download_ms: u64,
    /// Compiler wall time.
    pub compiler_ms: u64,
    /// MBX object handling.
    pub mbx_ms: u64,
    /// Test execution proper.
    pub test_ms: u64,
    /// Lock waits.
    pub lock_wait_ms: u64,
}

impl TaskTiming {
    /// Sum of separately measured slots.
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

    /// Parent-exclusive time: overlapped children never double-count.
    #[must_use]
    pub fn exclusive_ms(wall_ms: u64, children_union_ms: u64) -> u64 {
        wall_ms.saturating_sub(children_union_ms)
    }
}

/// Contract rendering of one scheduler timing breakdown (PAR-9.2).
#[must_use]
pub fn contract_timing(timing: &TaskTiming) -> velnor_actions_contract_workflow::TaskTiming {
    velnor_actions_contract_workflow::TaskTiming {
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

/// Timing breakdown for one measured task duration (PAR-9.2).
///
/// The obligation wrapper measures only the task-body wall, so the
/// measured duration lands in the `task_ms` slot and every other
/// slot reads zero; unmeasured durations stay absent, never
/// fabricated. Renders through [`contract_timing`] so scheduler and
/// contract math agree by construction.
#[must_use]
pub fn measured_timing(
    duration_ms: Option<u64>,
) -> Option<velnor_actions_contract_workflow::TaskTiming> {
    duration_ms.map(|elapsed| {
        contract_timing(&TaskTiming {
            task_ms: elapsed,
            ..TaskTiming::default()
        })
    })
}

/// Sum one timing breakdown over many tasks, slot by slot.
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

/// One owner per data path (cache §2): path prefix plus owning layer.
///
/// Roots mirror the renderer cache templates: tools, Cargo sources,
/// action-managed MBX objects, per-lane target dirs, and task artifacts.
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
