//! Parallel-Clippy memory grouping (par §7).
//!
//! Clippy configurations for one runner schedule in barrier-separated
//! waves when the runner cannot safely overlap them; every check is
//! retained in exactly one wave. Barriers sequence waves, never remove
//! checks. Without capacity data the planner stays conservative: each
//! distinct Clippy configuration gets its own wave.

use std::collections::BTreeSet;

use velnor_actions_contract_planning::ProposedTask;
use velnor_actions_rust::is_clippy_kind;

/// Barrier-separated Clippy schedule waves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClippyMemoryPlan {
    /// Task IDs per wave in schedule order; barriers sit between waves.
    pub groups: Vec<Vec<String>>,
    /// Barrier count between waves (`groups.len() - 1`, else 0).
    pub barriers: usize,
}

/// Schedule Clippy configurations in separate waves (par §7).
///
/// Non-Clippy tasks join the first wave; each distinct Clippy
/// configuration forms its own later wave. Input order is preserved
/// inside every wave and every input task lands in exactly one wave.
#[must_use]
pub fn clippy_memory_groups(tasks: &[ProposedTask]) -> ClippyMemoryPlan {
    if tasks.is_empty() {
        return ClippyMemoryPlan {
            groups: Vec::new(),
            barriers: 0,
        };
    }
    let mut configs: BTreeSet<&str> = BTreeSet::new();
    for task in tasks {
        if is_clippy_kind(&task.task_kind) {
            configs.insert(task.configuration.as_str());
        }
    }
    let waves: Vec<Option<&str>> = if configs.is_empty() {
        vec![None]
    } else {
        configs.into_iter().map(Some).collect()
    };
    let mut plan = Vec::with_capacity(waves.len());
    for (index, config) in waves.iter().enumerate() {
        let mut wave = Vec::new();
        for task in tasks {
            if wave_owns(task, *config, index == 0) {
                wave.push(task.task_id.clone());
            }
        }
        plan.push(wave);
    }
    ClippyMemoryPlan {
        barriers: plan.len().saturating_sub(1),
        groups: plan,
    }
}

/// True when `task` belongs in the wave for `config`.
///
/// The first wave also carries every non-Clippy task; later waves
/// carry only their configuration's Clippy tasks.
fn wave_owns(task: &ProposedTask, config: Option<&str>, first: bool) -> bool {
    if !is_clippy_kind(&task.task_kind) {
        return first;
    }
    config.is_some_and(|name| name == task.configuration.as_str())
}
#[cfg(test)]
mod tests;
