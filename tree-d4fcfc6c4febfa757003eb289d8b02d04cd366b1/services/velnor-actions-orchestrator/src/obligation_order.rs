use velnor_actions_contract::Stack;

/// Shared key for the order rendered into crate jobs and downstream reports.
pub(crate) fn obligation_order_key<'a>(
    stack_id: &str,
    task_kind: &str,
    task_id: &'a str,
) -> (u32, &'a str) {
    let rank = match Stack::from_id(stack_id) {
        Some(Stack::Tofu) => velnor_actions_tofu_core::task_kind_rank(task_kind),
        _ => velnor_actions_rust::task_kind_rank(task_kind),
    };
    (rank, task_id)
}
