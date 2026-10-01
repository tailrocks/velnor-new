//! Parallel-Clippy memory grouping (par §7).
//!
//! Clippy configurations for one runner schedule in barrier-separated
//! waves when the runner cannot safely overlap them; every check is
//! retained in exactly one wave. Barriers sequence waves, never remove
//! checks. Without capacity data the planner stays conservative: each
//! distinct Clippy configuration gets its own wave.

use std::collections::BTreeSet;

use velnor_actions_rust::{TaskGroup, TaskKind};

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
pub fn clippy_memory_groups(groups: &[TaskGroup]) -> ClippyMemoryPlan {
    if groups.is_empty() {
        return ClippyMemoryPlan {
            groups: Vec::new(),
            barriers: 0,
        };
    }
    let mut configs: BTreeSet<&str> = BTreeSet::new();
    for group in groups {
        if group.kind == TaskKind::Clippy {
            configs.insert(group.configuration.as_str());
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
        for group in groups {
            if wave_owns(group, *config, index == 0) {
                wave.push(group.task_id.clone());
            }
        }
        plan.push(wave);
    }
    ClippyMemoryPlan {
        barriers: plan.len().saturating_sub(1),
        groups: plan,
    }
}

/// True when `group` belongs in the wave for `config`.
///
/// The first wave also carries every non-Clippy task; later waves
/// carry only their configuration's Clippy tasks.
fn wave_owns(group: &TaskGroup, config: Option<&str>, first: bool) -> bool {
    if group.kind != TaskKind::Clippy {
        return first;
    }
    config.is_some_and(|name| name == group.configuration.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use velnor_actions_rust::{CompileDriver, NextestProfile, TestRunner};

    /// Minimal group with `kind`, `configuration`, and `task_id`.
    fn group(kind: TaskKind, configuration: &str, task_id: &str) -> TaskGroup {
        TaskGroup {
            task_id: task_id.to_owned(),
            package_id: "pkg".to_owned(),
            package_name: "pkg".to_owned(),
            manifest_key: "root".to_owned(),
            kind,
            configuration: configuration.to_owned(),
            features: Vec::new(),
            target: "host".to_owned(),
            gated_by: Vec::new(),
            depends_on: Vec::new(),
            target_flags: Vec::new(),
            no_test_targets: false,
            package_arg: None,
            compile_driver: CompileDriver::Cargo,
            test_runner: TestRunner::CargoTest,
            declared_inputs: Vec::new(),
            undeclared_reads: false,
            uses_network: false,
            uses_clock: false,
            uses_random: false,
            nextest_profile: NextestProfile::Default,
        }
    }

    #[test]
    fn distinct_clippy_configs_separate_with_barrier() {
        let groups = vec![
            group(TaskKind::Clippy, "default", "clippy-default"),
            group(TaskKind::Clippy, "all-features", "clippy-all"),
            group(TaskKind::Test, "default", "test-default"),
        ];
        let plan = clippy_memory_groups(&groups);
        assert_eq!(plan.barriers, 1);
        assert_eq!(plan.groups.len(), 2);
        assert_eq!(
            plan.groups[0],
            vec!["clippy-all".to_owned(), "test-default".to_owned()]
        );
        assert_eq!(plan.groups[1], vec!["clippy-default".to_owned()]);
        let mut union: Vec<&str> = plan.groups.iter().flatten().map(String::as_str).collect();
        union.sort_unstable();
        assert_eq!(union, vec!["clippy-all", "clippy-default", "test-default"]);
    }

    #[test]
    fn single_config_and_empty_need_no_barrier() {
        let groups = vec![
            group(TaskKind::Clippy, "default", "clippy"),
            group(TaskKind::Test, "default", "test"),
        ];
        let plan = clippy_memory_groups(&groups);
        assert_eq!(plan.barriers, 0);
        assert_eq!(
            plan.groups,
            vec![vec!["clippy".to_owned(), "test".to_owned()]]
        );
        let empty = clippy_memory_groups(&[]);
        assert_eq!(empty.barriers, 0);
        assert!(empty.groups.is_empty());
        let no_clippy = clippy_memory_groups(&[group(TaskKind::Test, "default", "test")]);
        assert_eq!(no_clippy.barriers, 0);
        assert_eq!(no_clippy.groups.len(), 1);
    }
}
