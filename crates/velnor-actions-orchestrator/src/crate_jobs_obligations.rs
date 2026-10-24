//! Ordered, validated obligations for one crate job.

use std::collections::BTreeSet;

use velnor_actions_contract::workflow::TaskBindingDigest;
use velnor_actions_contract::{
    CrateObligation, ProposedTask, matrix_id_for_task_group, matrix_key_for_id,
};
use velnor_actions_mise::ToolCatalog;

use crate::OrchestratorError;
use crate::matrix_step::step_name_for;
use crate::obligation_order::obligation_order_key;

/// Rank one proposal through the shared task-order key.
#[cfg(test)]
pub(super) fn obligation_rank(task: &ProposedTask) -> u32 {
    obligation_order_key(&task.stack_id, &task.task_kind, &task.task_id).0
}

/// Ordered validated obligations for one crate's tasks.
///
/// In Rust jobs same-job data producers come first, so every gate references
/// a strictly earlier obligation and the remaining checks form one
/// contiguous parallel group.
pub(super) fn obligations_for(
    tasks: &[&ProposedTask],
    catalog: &ToolCatalog,
) -> Result<Vec<CrateObligation>, OrchestratorError> {
    let executed: BTreeSet<&str> = tasks.iter().map(|task| task.task_id.as_str()).collect();
    // Rust jobs run checks after their executable builds; OpenTofu stays serial.
    let producers: BTreeSet<&str> = if tasks.iter().any(|task| super::is_opentofu(task)) {
        BTreeSet::new()
    } else {
        tasks
            .iter()
            .filter(|task| velnor_actions_rust::runs_before_parallel_checks(&task.task_kind))
            .map(|task| task.task_id.as_str())
            .collect()
    };
    let mut ordered = tasks.to_vec();
    ordered.sort_by(|left, right| {
        (
            !producers.contains(left.task_id.as_str()),
            obligation_order_key(&left.stack_id, &left.task_kind, &left.task_id),
        )
            .cmp(&(
                !producers.contains(right.task_id.as_str()),
                obligation_order_key(&right.stack_id, &right.task_kind, &right.task_id),
            ))
    });
    let mut obligations = Vec::with_capacity(ordered.len());
    for task in ordered {
        obligations.push(obligation_for(task, &executed, catalog)?);
    }
    Ok(obligations)
}

/// One obligation: identities, same-job ordered gates, fixed argv.
fn obligation_for(
    task: &ProposedTask,
    executed: &BTreeSet<&str>,
    catalog: &ToolCatalog,
) -> Result<CrateObligation, OrchestratorError> {
    let argv = crate::vectors::task_argv(task, catalog)?;
    let toolchain_inputs = crate::internal_plan::identities::toolchain_inputs_for(task, catalog)?;
    let toolchain = velnor_actions_contract::cachekey::toolchain_id(&toolchain_inputs)?;
    let digest = TaskBindingDigest::new(crate::internal::plan_obligation::task_digest(
        &task.task_id,
        &argv,
        &toolchain,
    )?)?;
    let matrix_id = matrix_id_for_task_group(&task.stack_id, &task.task_id)?;
    let matrix_key = matrix_key_for_id(&matrix_id)?;
    Ok(CrateObligation {
        task_id: task.task_id.clone(),
        configuration: task.configuration.clone(),
        kind: task.task_kind.clone(),
        step_name: step_name_for(&task.task_kind, &task.task_id),
        gated_by: gates_for(task, executed),
        matrix_key,
        task_digest: digest,
        toolchain_inputs,
        run: argv,
    })
}

/// Sorted same-job gates: quality gates plus data producers.
///
/// Gates naming skipped tasks (test-less doctests) are vacuous: the
/// kind order still sequences the survivors, and dangling references
/// would fail the strictly-earlier validation.
pub(super) fn gates_for(task: &ProposedTask, executed: &BTreeSet<&str>) -> Vec<String> {
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
