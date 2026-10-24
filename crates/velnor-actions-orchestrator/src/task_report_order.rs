//! Same-job execution ordering for downstream report synthesis.

use velnor_actions_contract::{ExecuteTaskRef, Plan, Stack};

/// IDs actually skipped after a failed sequential obligation in one job.
pub(super) fn derive_downstream(plan: &Plan, task_id: &str, job_id: &str) -> Vec<String> {
    let mut ordered = Vec::new();
    let mut rust_job = true;
    for entry in &plan.matrix.include {
        if entry.job_id != job_id {
            continue;
        }
        rust_job &= entry.stack_id == Stack::Rust.id();
        for (kind, task_ref) in &entry.execute_task_ids.tasks {
            let ids: &[String] = match task_ref {
                ExecuteTaskRef::Single(id) => std::slice::from_ref(id),
                ExecuteTaskRef::Shards(shards) => shards,
            };
            for id in ids {
                ordered.push((
                    crate::obligation_order::obligation_order_key(&entry.stack_id, kind, id),
                    kind.as_str(),
                ));
            }
        }
    }
    if rust_job {
        ordered.sort_unstable_by_key(|(key, kind)| {
            (
                !velnor_actions_rust::runs_before_parallel_checks(kind),
                *key,
            )
        });
    } else {
        ordered.sort_unstable();
    }
    let Some(current) = ordered.iter().position(|((_, id), _)| *id == task_id) else {
        return Vec::new();
    };
    if rust_job && !velnor_actions_rust::runs_before_parallel_checks(ordered[current].1) {
        return Vec::new();
    }
    ordered[current + 1..]
        .iter()
        .map(|((_, id), _)| (*id).to_owned())
        .collect()
}
