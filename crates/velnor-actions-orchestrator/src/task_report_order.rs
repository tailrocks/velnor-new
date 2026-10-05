//! Same-job execution ordering for downstream report synthesis.

use velnor_actions_contract::{ExecuteTaskRef, Plan, Stack};

/// Shared key for the order rendered into crate jobs and downstream reports.
pub(crate) fn obligation_order_key<'a>(
    stack_id: &str,
    task_kind: &str,
    task_id: &'a str,
) -> (u32, &'a str) {
    let rank = match Stack::from_id(stack_id) {
        Some(Stack::Tofu) => velnor_actions_tofu::task_kind_rank(task_kind),
        _ => velnor_actions_rust::task_kind_rank(task_kind),
    };
    (rank, task_id)
}

/// Downstream obligation IDs in the crate job's execution order.
pub(super) fn derive_downstream(plan: &Plan, task_id: &str, job_id: &str) -> Vec<String> {
    let mut ordered = Vec::new();
    for entry in &plan.matrix.include {
        if entry.job_id != job_id {
            continue;
        }
        for (kind, task_ref) in &entry.execute_task_ids.tasks {
            match task_ref {
                ExecuteTaskRef::Single(id) => {
                    ordered.push(obligation_order_key(&entry.stack_id, kind, id));
                }
                ExecuteTaskRef::Shards(ids) => {
                    ordered.extend(
                        ids.iter()
                            .map(|id| obligation_order_key(&entry.stack_id, kind, id)),
                    );
                }
            }
        }
    }
    ordered.sort_unstable();
    let Some(current) = ordered.iter().position(|(_, id)| *id == task_id) else {
        return Vec::new();
    };
    ordered[current + 1..]
        .iter()
        .map(|(_, id)| (*id).to_owned())
        .collect()
}
