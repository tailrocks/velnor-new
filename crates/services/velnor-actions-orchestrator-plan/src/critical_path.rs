//! End-to-end critical path over the obligation DAG (par §2).
//!
//! Longest duration-weighted chain through task dependencies, plus the
//! per-task durations along it. Plan-time calls pass no durations and
//! get the structural path; event-time calls pass measured durations.

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract_planning::ProposedTask;

/// Longest chain through the obligation DAG.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CriticalPath {
    /// Task IDs from chain head to tail.
    pub path: Vec<String>,
    /// Summed durations along the chain in milliseconds.
    pub total_duration_ms: u64,
}

/// Longest duration-weighted chain over `durations` and `predecessors`.
///
/// Predecessors absent from `durations` are external roots: satisfied
/// with zero duration and excluded from the path. Ties resolve to the
/// lexicographically smallest chain head; back edges are ignored so a
/// cyclic input still terminates deterministically.
#[must_use]
pub fn critical_path(
    durations: &BTreeMap<String, u64>,
    predecessors: &BTreeMap<String, Vec<String>>,
) -> CriticalPath {
    let mut memo: BTreeMap<&str, (u64, Vec<String>)> = BTreeMap::new();
    let mut best: (u64, Vec<String>) = (0, Vec::new());
    for id in durations.keys() {
        let mut visiting = BTreeSet::new();
        let candidate = chain_through(id, durations, predecessors, &mut memo, &mut visiting);
        if candidate.0 > best.0
            || (candidate.0 == best.0 && lexicographically_smaller(&candidate.1, &best.1))
        {
            best = candidate;
        }
    }
    CriticalPath {
        path: best.1,
        total_duration_ms: best.0,
    }
}

/// Critical path for derived tasks; missing durations count as zero.
#[must_use]
pub fn critical_path_for_groups(
    tasks: &[ProposedTask],
    durations: &BTreeMap<String, u64>,
) -> CriticalPath {
    let mut weights = BTreeMap::new();
    let mut edges: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for task in tasks {
        weights.insert(
            task.task_id.clone(),
            durations.get(&task.task_id).copied().unwrap_or(0),
        );
        let mut preds: BTreeSet<&str> = BTreeSet::new();
        for dep in task.gated_by.iter().chain(task.depends_on.iter()) {
            preds.insert(dep.as_str());
        }
        edges.insert(
            task.task_id.clone(),
            preds.into_iter().map(str::to_owned).collect(),
        );
    }
    critical_path(&weights, &edges)
}

/// Structural critical path: every task weighs one, so the longest
/// dependency chain wins. Used where durations are not yet measured.
#[must_use]
pub fn critical_path_structural(tasks: &[ProposedTask]) -> CriticalPath {
    let durations: BTreeMap<String, u64> =
        tasks.iter().map(|task| (task.task_id.clone(), 1)).collect();
    critical_path_for_groups(tasks, &durations)
}

/// One plan-text line for the structural critical path (no durations).
#[must_use]
pub fn critical_path_line(path: &CriticalPath) -> String {
    if path.path.is_empty() {
        return "Critical path: none (no obligations)".to_owned();
    }
    format!(
        "Critical path: {} ({} tasks)",
        path.path.join(" -> "),
        path.path.len()
    )
}

/// Full critical-path report lines: chain, total, and task durations.
#[must_use]
pub fn render_critical_path(path: &CriticalPath, durations: &BTreeMap<String, u64>) -> Vec<String> {
    let mut lines = vec![format!(
        "Critical path: {} ({} tasks, total {} ms)",
        if path.path.is_empty() {
            "none".to_owned()
        } else {
            path.path.join(" -> ")
        },
        path.path.len(),
        path.total_duration_ms
    )];
    for id in &path.path {
        lines.push(format!(
            "  {id}: {} ms",
            durations.get(id).copied().unwrap_or(0)
        ));
    }
    lines
}

/// Longest chain ending at `id`: total plus head-to-tail task IDs.
fn chain_through<'a>(
    id: &'a str,
    durations: &'a BTreeMap<String, u64>,
    predecessors: &BTreeMap<String, Vec<String>>,
    memo: &mut BTreeMap<&'a str, (u64, Vec<String>)>,
    visiting: &mut BTreeSet<&'a str>,
) -> (u64, Vec<String>) {
    if let Some(known) = memo.get(id) {
        return known.clone();
    }
    if !visiting.insert(id) {
        return (0, Vec::new());
    }
    let mut best: (u64, Vec<String>) = (0, Vec::new());
    if let Some(preds) = predecessors.get(id) {
        let mut sorted = preds.clone();
        sorted.sort();
        for pred in &sorted {
            let Some(key) = durations.get_key_value(pred).map(|(key, _)| key.as_str()) else {
                continue;
            };
            let candidate = chain_through(key, durations, predecessors, memo, visiting);
            if candidate.0 > best.0
                || (candidate.0 == best.0 && lexicographically_smaller(&candidate.1, &best.1))
            {
                best = candidate;
            }
        }
    }
    visiting.remove(id);
    let total = best
        .0
        .saturating_add(durations.get(id).copied().unwrap_or(0));
    best.1.push(id.to_owned());
    memo.insert(id, (total, best.1.clone()));
    (total, best.1)
}

/// True when `left` is the smaller chain, preferring nonempty on ties.
fn lexicographically_smaller(left: &[String], right: &[String]) -> bool {
    match (left.is_empty(), right.is_empty()) {
        (false, true) => true,
        (true, false) => false,
        _ => left < right,
    }
}
#[cfg(test)]
mod tests;
