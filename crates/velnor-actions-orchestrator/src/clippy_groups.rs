use std::collections::BTreeSet;

use velnor_actions_contract::ProposedTask;
use velnor_actions_rust::is_clippy_kind;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClippyMemoryPlan {
    pub groups: Vec<Vec<String>>,
    pub barriers: usize,
}

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

fn wave_owns(task: &ProposedTask, config: Option<&str>, first: bool) -> bool {
    if !is_clippy_kind(&task.task_kind) {
        return first;
    }
    config.is_some_and(|name| name == task.configuration.as_str())
}
