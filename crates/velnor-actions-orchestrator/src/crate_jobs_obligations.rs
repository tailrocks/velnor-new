//! Ordered crate obligation construction and same-crate gates.

use std::collections::BTreeSet;

use velnor_actions_contract::{
    CrateObligation, ProposedTask, Stack, matrix_id_for_task_group, matrix_key_for_id,
};
use velnor_actions_mise::ToolCatalog;
use velnor_actions_rust::task_kind_rank;

use crate::OrchestratorError;
use crate::matrix_step::step_name_for;

/// Obligation order rank for one task, dispatched by stack.
pub(crate) fn obligation_rank(task: &ProposedTask) -> u32 {
    match Stack::from_id(&task.stack_id) {
        Some(Stack::Tofu) => velnor_actions_tofu::task_kind_rank(&task.task_kind),
        _ => task_kind_rank(&task.task_kind),
    }
}

/// Ordered validated obligations for one crate's tasks.
pub(crate) fn obligations_for(
    tasks: &[&ProposedTask],
    catalog: &ToolCatalog,
) -> Result<Vec<CrateObligation>, OrchestratorError> {
    let executed: BTreeSet<&str> = tasks.iter().map(|task| task.task_id.as_str()).collect();
    let mut ordered = tasks.to_vec();
    ordered.sort_by(|left, right| {
        (obligation_rank(left), &left.task_id).cmp(&(obligation_rank(right), &right.task_id))
    });
    let mut obligations = Vec::with_capacity(ordered.len());
    for task in ordered {
        obligations.push(obligation_for(task, &executed, catalog)?);
    }
    Ok(obligations)
}

/// One obligation: identities, same-crate gates, fixed argv.
fn obligation_for(
    task: &ProposedTask,
    executed: &BTreeSet<&str>,
    catalog: &ToolCatalog,
) -> Result<CrateObligation, OrchestratorError> {
    let argv = crate::vectors::task_argv(task, catalog)?;
    let toolchain = crate::internal_plan::toolchain_id(task, catalog)?;
    let digest = crate::internal::plan_obligation::task_digest(&task.task_id, &argv, &toolchain)?;
    let matrix_id = matrix_id_for_task_group(&task.stack_id, &task.task_id)?;
    let matrix_key = matrix_key_for_id(&matrix_id)?;
    Ok(CrateObligation {
        task_id: task.task_id.clone(),
        kind: task.task_kind.clone(),
        step_name: step_name_for(&task.task_kind, &task.task_id),
        gated_by: gates_for(task, executed),
        matrix_key,
        task_digest: digest,
        run: argv,
    })
}

/// Sorted same-crate gates: quality gates plus data producers.
///
/// Gates naming skipped tasks (test-less doctests) are vacuous: the
/// kind order still sequences survivors; dangling references fail validation.
pub(crate) fn gates_for(task: &ProposedTask, executed: &BTreeSet<&str>) -> Vec<String> {
    let mut gates: Vec<String> = task
        .gated_by
        .iter()
        .chain(task.depends_on.iter())
        .filter(|gate| executed.contains(gate.as_str()))
        .cloned()
        .collect();
    gates.sort();
    gates.dedup();
    gates
}
