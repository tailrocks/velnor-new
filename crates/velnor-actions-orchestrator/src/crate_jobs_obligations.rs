// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::cachekey::SelectedTestTool;
use velnor_actions_contract::{
    CrateObligation, ProposedTask, matrix_id_for_task_group, matrix_key_for_id,
};
use velnor_actions_mise::ToolCatalog;

use crate::OrchestratorError;
use crate::matrix_step::step_name_for;
use crate::obligation_order::obligation_order_key;

pub(super) fn obligations_for(
    tasks: &[&ProposedTask],
    catalog: &ToolCatalog,
    test_tools: &BTreeMap<String, Vec<SelectedTestTool>>,
) -> Result<Vec<CrateObligation>, OrchestratorError> {
    let executed: BTreeSet<&str> = tasks.iter().map(|task| task.task_id.as_str()).collect();
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
        obligations.push(obligation_for(task, &executed, catalog, test_tools)?);
    }
    Ok(obligations)
}

fn obligation_for(
    task: &ProposedTask,
    executed: &BTreeSet<&str>,
    catalog: &ToolCatalog,
    test_tools: &BTreeMap<String, Vec<SelectedTestTool>>,
) -> Result<CrateObligation, OrchestratorError> {
    let argv = crate::vectors::task_argv(task, catalog)?;
    let mut toolchain_inputs =
        crate::internal_plan::identities::toolchain_inputs_for(task, catalog)?;
    if crate::crate_test_tools::is_supported_test_task(task)
        && let Some(selected) = test_tools.get(&task.display_name)
    {
        toolchain_inputs.selected_test_tools.clone_from(selected);
    }
    let toolchain = velnor_actions_contract::cachekey::toolchain_id(&toolchain_inputs)?;
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
        toolchain_inputs,
        run: argv,
    })
}

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
