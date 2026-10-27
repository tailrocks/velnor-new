// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

use velnor_actions_contract::workflow::TaskExecutionManifestV1;
use velnor_actions_contract::{ExecuteTaskRef, JobDisposition, Plan, task_report_id_for_task};

use crate::{
    OrchestratorError,
    internal::{internal, internal_contract},
};

use super::super::emit_read::{read_staged_report, read_staged_sidecar, unit_result_for_report};
use super::{EmitEnv, OrderedUnits, obligation_digest, split_job_key};

pub(super) fn build_ordered_results(
    plan: &Plan,
    manifest: &TaskExecutionManifestV1,
    run_key: &str,
    emit: &EmitEnv,
    run_dir: &Path,
    finished_secs: u64,
) -> Result<OrderedUnits, OrchestratorError> {
    let (group, _) = split_job_key(&emit.job_key)?;
    if !plan
        .jobs
        .iter()
        .any(|job| job.job_id == group && job.disposition == JobDisposition::Run)
    {
        return Err(internal("ledger_group_not_selected"));
    }
    let mut results = BTreeMap::new();
    for entry in &plan.matrix.include {
        if entry.job_id != group
            || emit
                .matrix_key
                .as_ref()
                .is_some_and(|key| *key != entry.matrix_key)
        {
            continue;
        }
        let mut units = BTreeSet::new();
        for tasks in entry.execute_task_ids.tasks.values() {
            match tasks {
                ExecuteTaskRef::Single(id) => {
                    units.insert(id.clone());
                }
                ExecuteTaskRef::Shards(ids) => units.extend(ids.iter().cloned()),
            }
        }
        let tasks_dir = run_dir.join(&entry.matrix_key).join("tasks");
        for unit_id in units {
            let task = manifest
                .tasks
                .get(&unit_id)
                .ok_or_else(|| internal(&format!("task_without_manifest_entry:{unit_id}")))?;
            let digest = obligation_digest(plan, &unit_id)?;
            let report_id = task_report_id_for_task(run_key, &entry.matrix_key, digest)
                .map_err(internal_contract)?;
            let staged = read_staged_report(&tasks_dir, &report_id)?;
            let sidecar = read_staged_sidecar(&tasks_dir, &report_id)?;
            let result = unit_result_for_report(
                &unit_id,
                task,
                &report_id,
                staged.as_deref(),
                emit.job_status,
                finished_secs,
            )?;
            if results.insert(unit_id, (result, sidecar)).is_some() {
                return Err(internal("duplicate_group_unit"));
            }
        }
    }
    if results.is_empty() {
        return Err(internal("matrix_key_not_in_group"));
    }
    Ok(results.into_values().unzip())
}
