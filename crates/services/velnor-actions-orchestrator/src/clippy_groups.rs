//! Parallel-Clippy memory grouping (par §7).
//!
//! Clippy configurations for one runner schedule in barrier-separated
//! waves when the runner cannot safely overlap them; every check is
//! retained in exactly one wave. Barriers sequence waves, never remove
//! checks. Without capacity data the planner stays conservative: each
//! distinct Clippy configuration gets its own wave.

use std::collections::BTreeSet;

use velnor_actions_contract::ProposedTask;
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
mod tests {
    use super::*;
    use velnor_actions_contract::{CachePolicy, IdentityInputs, ResourceClass, ResourceDemand};

    /// Minimal proposal with `kind`, `configuration`, and `task_id`.
    fn task(kind: &str, configuration: &str, task_id: &str) -> ProposedTask {
        ProposedTask {
            task_id: task_id.to_owned(),
            stack_id: "rust".to_owned(),
            component_id: "pkg".to_owned(),
            task_kind: kind.to_owned(),
            configuration: configuration.to_owned(),
            depends_on: Vec::new(),
            gated_by: Vec::new(),
            reads: Vec::new(),
            writes: Vec::new(),
            outputs: Vec::new(),
            resource: ResourceDemand {
                class: ResourceClass::Compiler,
                cpu_milli: None,
                memory_mb: None,
                needs_network: false,
                service: None,
            },
            cache_policy: CachePolicy {
                allow_compilation_reuse: true,
                allow_task_reuse: true,
            },
            identity: IdentityInputs {
                unit_id: "pkg".to_owned(),
                unit_key: "root".to_owned(),
                unit_path: "Cargo.toml".to_owned(),
                project_root: ".".to_owned(),
                target: "host".to_owned(),
                features: Vec::new(),
                flags: Vec::new(),
                compile_driver: "cargo".to_owned(),
                test_runner: "cargo_test".to_owned(),
                environment: std::collections::BTreeMap::new(),
                declared_inputs: Vec::new(),
                undeclared_reads: false,
            },
            payload: vec![std::ffi::OsString::from("clippy")],
            display_name: "pkg".to_owned(),
            uses_clock: false,
            uses_random: false,
            no_targets: false,
            runner_profile: "default".to_owned(),
        }
    }

    #[test]
    fn distinct_clippy_configs_separate_with_barrier() {
        let groups = vec![
            task("clippy", "default", "clippy-default"),
            task("clippy", "all-features", "clippy-all"),
            task("test", "default", "test-default"),
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
            task("clippy", "default", "clippy"),
            task("test", "default", "test"),
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
        let no_clippy = clippy_memory_groups(&[task("test", "default", "test")]);
        assert_eq!(no_clippy.barriers, 0);
        assert_eq!(no_clippy.groups.len(), 1);
    }
}
