use super::{GroupInputs, OrchestratorError, internal, internal_contract, wire_w2};
use crate::internal_plan::snapshot::platform_target_for_group;
use crate::internal_plan::{
    adapter_metadata, cache_ids_for, evidence_for_group, execute_ids, record_task_cache,
};
use velnor_actions_contract::workflow::TaskBindingDigest;
use velnor_actions_contract::{
    MatrixEntry, NamedCheckLane, PlanObligation, PlannedPlatform, ProposedTask,
};

/// Digest the selected MBX cache generation.
fn mbx_identity_digest(catalog: &velnor_actions_mise::ToolCatalog) -> String {
    let generation = velnor_actions_contract::cachekey::mbx_cache_generation(
        catalog.version(velnor_actions_mise::PinnedTool::MrBoxington),
    );
    velnor_actions_contract::digest_b3(generation.as_bytes())
}

pub(super) struct GroupCompletion<'a> {
    pub(super) argv: &'a [String],
    pub(super) task_binding_digest: TaskBindingDigest,
    pub(super) toolchain: &'a str,
    pub(super) toolchain_inputs: &'a velnor_actions_contract::cachekey::ToolchainInputs,
    pub(super) input_digest: &'a str,
    pub(super) closure_digest: String,
    pub(super) graph_digest: String,
    pub(super) reuse: wire_w2::ReuseOutcome,
}

/// Complete existing matrix transport after execution disposition is decided.
pub(super) fn complete_group(
    inputs: &GroupInputs<'_>,
    completion: GroupCompletion<'_>,
) -> Result<(PlanObligation, Vec<MatrixEntry>), OrchestratorError> {
    let task = inputs.task;
    let task_digest = super::task_digest(&task.task_id, completion.argv, completion.toolchain)
        .map_err(internal_contract)?;
    // The persisted input digest flows through the validated reuse
    // outcome when one exists, so merge-time live comparison judges the
    // exact recorded value; forced-execution paths carry the identity.
    let recorded = completion
        .reuse
        .recorded_input_digest
        .clone()
        .unwrap_or_else(|| completion.input_digest.to_owned());
    let Some(job_id) = inputs.job_owners.for_member(task, inputs.wire.event)? else {
        return Err(internal("crate_job_id_missing"));
    };
    let obligation = PlanObligation {
        task_id: task.task_id.clone(),
        decision: completion.reuse.decision,
        reason: completion.reuse.reason,
        task_digest: task_digest.clone(),
        task_binding_digest: completion.task_binding_digest,
        input_digest: recorded,
        closure_digest: completion.closure_digest,
        graph_digest: completion.graph_digest,
        mbx_digest: mbx_identity_digest(inputs.catalog),
        profile: task.configuration.clone(),
        job_id: job_id.clone(),
    };
    let mut metadata = adapter_metadata(task, evidence_for_group(inputs.discovery, task))
        .map_err(internal_contract)?;
    record_task_cache(
        &mut metadata,
        completion.reuse.task_cache_enabled,
        completion.reuse.task_cache_key.as_deref(),
    );
    let run = velnor_actions_workflow_renderer::join_argv_for_run(completion.argv)
        .map_err(|err| internal(&err.to_string()))?;
    let job_id = inputs
        .job_owners
        .for_member(task, inputs.wire.event)
        .map_err(internal_contract)?
        .ok_or_else(|| internal("crate_job_id_missing"))?;
    let lanes = inputs
        .named_check_lanes
        .get(&job_id)
        .cloned()
        .unwrap_or_else(|| {
            vec![NamedCheckLane {
                variant: None,
                job_id,
                runs_on: Some(inputs.label.to_owned()),
            }]
        });
    if lanes.is_empty() {
        return Err(internal("qualification_executor_lane_missing"));
    }
    let target = platform_target_for_group(inputs.label, task).map_err(internal_contract)?;
    let entries = derive_lane_entries(
        &LaneEntryInputs {
            task,
            run: &run,
            task_digest: &task_digest,
            metadata: &metadata,
            execute_task_ids: &execute_ids(task),
            input_digest: completion.input_digest,
            run_key: inputs.run_key,
            target: &target,
            default_runs_on: inputs.label,
            toolchain: completion.toolchain,
            toolchain_inputs: completion.toolchain_inputs,
        },
        lanes,
    )?;
    Ok((obligation, entries))
}

/// Inputs shared by every lane derived for one obligation.
pub(super) struct LaneEntryInputs<'a> {
    pub(super) task: &'a ProposedTask,
    pub(super) run: &'a str,
    pub(super) task_digest: &'a str,
    pub(super) metadata: &'a serde_json::Value,
    pub(super) execute_task_ids: &'a velnor_actions_contract::ExecuteTaskIds,
    pub(super) input_digest: &'a str,
    pub(super) run_key: &'a str,
    pub(super) target: &'a str,
    pub(super) default_runs_on: &'a str,
    pub(super) toolchain: &'a str,
    pub(super) toolchain_inputs: &'a velnor_actions_contract::cachekey::ToolchainInputs,
}

/// Derive the exact persisted `MatrixEntry` identities for physical lanes.
pub(super) fn derive_lane_entries(
    inputs: &LaneEntryInputs<'_>,
    lanes: Vec<NamedCheckLane>,
) -> Result<Vec<MatrixEntry>, OrchestratorError> {
    let mut entries = Vec::with_capacity(lanes.len());
    for lane in lanes {
        let runs_on = lane.runs_on.as_deref().unwrap_or(inputs.default_runs_on);
        let planned_platform =
            PlannedPlatform::new(runs_on, inputs.target).map_err(internal_contract)?;
        let platform_id = planned_platform.platform_id.clone();
        let mut entry = MatrixEntry::derive_for_lane(
            &inputs.task.stack_id,
            &inputs.task.task_id,
            inputs.run,
            inputs.task_digest,
            inputs.metadata.clone(),
            inputs.execute_task_ids.clone(),
            inputs.input_digest,
            inputs.run_key,
            &lane.job_id,
            lane.variant,
            planned_platform,
        )
        .map_err(internal_contract)?;
        let cache_ids = if velnor_actions_contract::RunsOn::parse(runs_on)
            .map_err(internal_contract)?
            .is_scale_set()
        {
            crate::internal_plan::cache_ids_for_platform(
                inputs.task,
                inputs.toolchain,
                &platform_id,
            )
        } else {
            cache_ids_for(inputs.task, runs_on, inputs.toolchain)
        }
        .map_err(internal_contract)?;
        record_lane_target_dir(&mut entry.adapter_metadata, cache_ids.lane_id());
        entry.cache_ids = Some(cache_ids);
        entry.toolchain_inputs = Some(inputs.toolchain_inputs.clone());
        entry.toolchain_id = Some(inputs.toolchain.to_owned());
        entries.push(entry);
    }
    Ok(entries)
}

/// Record the lane's isolated `CARGO_TARGET_DIR` on entry metadata.
///
/// Generated legs read this path for their step env; the value derives
/// from the responsibility-based lane digest via `target_dir_for_lane`.
fn record_lane_target_dir(metadata: &mut serde_json::Value, lane_id: &str) {
    let Some(object) = metadata.as_object_mut() else {
        return;
    };
    object.insert(
        "cargo_target_dir".to_owned(),
        serde_json::Value::String(crate::internal_plan::target_dir_for_lane_id(lane_id)),
    );
}

#[cfg(test)]
#[path = "complete_tests.rs"]
mod tests;
