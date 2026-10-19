//! Named checks complete through the existing obligation and matrix envelopes.
use crate::OrchestratorError;
use crate::internal::{internal, internal_contract};
use crate::internal_plan::identities::platform_id_for_group;
use crate::internal_plan::named_checks::toolchain_inputs;
use crate::internal_plan::snapshot::canonical_digest;
use crate::internal_plan::snapshot::platform_target_for_group;
use crate::internal_plan::{IdentityInputs, execute_ids, task_identity_digest};
use std::path::Path;
use velnor_actions_contract::{
    MatrixEntry, NamedCheckLane, ObligationDecision, PlanGenerator, PlanObligation, PlannedPlatform,
};
use velnor_actions_mise::{DiscoveredCheck, ToolCatalog};

/// Shared plan-time and runtime derivation; no detector or Cargo inventory required.
#[cfg(test)]
pub(crate) fn derive(
    root: &Path,
    item: &DiscoveredCheck,
    run_key: &str,
    generator: &PlanGenerator,
    catalog: &ToolCatalog,
) -> Result<(PlanObligation, MatrixEntry), OrchestratorError> {
    let job_id = format!("check-{}", item.check.id);
    let lane = NamedCheckLane {
        variant: None,
        job_id,
        runs_on: None,
    };
    let (obligation, mut entries) = derive_lanes(root, item, run_key, generator, catalog, &[lane])?;
    let entry = entries
        .pop()
        .ok_or_else(|| internal("named_check_lane_missing"))?;
    if !entries.is_empty() {
        return Err(internal("named_check_single_lane_mismatch"));
    }
    Ok((obligation, entry))
}

/// Derive one logical obligation and one matrix entry for each emitted lane.
pub(crate) fn derive_lanes(
    root: &Path,
    item: &DiscoveredCheck,
    run_key: &str,
    generator: &PlanGenerator,
    catalog: &ToolCatalog,
    lanes: &[NamedCheckLane],
) -> Result<(PlanObligation, Vec<MatrixEntry>), OrchestratorError> {
    derive_lanes_until(root, item, run_key, generator, catalog, lanes, None)
}

/// Derive lane entries and source identities against the same deadline as execution.
pub(crate) fn derive_lanes_until(
    root: &Path,
    item: &DiscoveredCheck,
    run_key: &str,
    generator: &PlanGenerator,
    catalog: &ToolCatalog,
    lanes: &[NamedCheckLane],
    deadline: Option<velnor_actions_mise::CheckDeadline>,
) -> Result<(PlanObligation, Vec<MatrixEntry>), OrchestratorError> {
    if lanes.is_empty() {
        return Err(internal("named_check_lane_missing"));
    }
    let task = &item.proposal;
    let argv = crate::vectors::task_argv(task, catalog)?;
    let toolchain_inputs = toolchain_inputs(task, catalog).map_err(internal_contract)?;
    let toolchain = velnor_actions_contract::cachekey::toolchain_id(&toolchain_inputs)
        .map_err(internal_contract)?;
    let platform =
        platform_id_for_group(&item.check.runner.label, task).map_err(internal_contract)?;
    let logical_platform = planned_platform_for_check(&item.check.runner.label, task)?;
    let graph =
        canonical_digest(&(&item.check.id, &item.check.directory)).map_err(internal_contract)?;
    let mut closure =
        super::resolve_closure_until(root, task, &graph, &toolchain, &platform, deadline)
            .map_err(internal_contract)?;
    closure.inputs.insert(
        "task_definition".to_owned(),
        velnor_actions_contract::Provenance::Known {
            digest: canonical_digest(&item.check).map_err(internal_contract)?,
        },
    );
    closure.inputs.insert(
        "resolved_task_config".to_owned(),
        velnor_actions_contract::Provenance::Known {
            digest: velnor_actions_contract::digest_b3(item.task_config.as_bytes()),
        },
    );
    let closure_digest = canonical_digest(&closure).map_err(internal_contract)?;
    let input_digest = task_identity_digest(&IdentityInputs {
        task,
        argv: &argv,
        toolchain_id: &toolchain,
        platform_id: &platform,
        manifest: &task.identity.unit_path,
        generator,
        extension: super::extension_for(item).map_err(internal_contract)?,
        closure_digest: &closure_digest,
    })
    .map_err(internal_contract)?;
    let task_digest =
        crate::internal::plan_obligation::task_digest(&task.task_id, &argv, &toolchain)
            .map_err(internal_contract)?;
    let task_binding_digest = crate::tool_catalog_project::task_binding_digest(task, catalog)?;
    let obligation = build_obligation(&BuildObligationInput {
        item,
        task_digest: task_digest.clone(),
        task_binding_digest,
        input_digest: input_digest.clone(),
        closure_digest,
        graph_digest: graph,
        catalog,
        lanes,
    })?;
    let run = velnor_actions_workflow_renderer::join_argv_for_run(&argv)
        .map_err(|e| internal(&e.to_string()))?;
    let entries = derive_entries(
        item,
        &run,
        lanes,
        &NamedCheckEntryContext {
            task_digest: &task_digest,
            input_digest: &input_digest,
            run_key,
            toolchain_inputs: &toolchain_inputs,
            toolchain_id: &toolchain,
            logical_platform: &logical_platform,
        },
    )?;
    Ok((obligation, entries))
}

/// Build the shared obligation for one or more named-check lanes.
///
/// # Errors
///
/// Returns an error when no lane owns the obligation.
struct BuildObligationInput<'a> {
    item: &'a DiscoveredCheck,
    task_digest: String,
    task_binding_digest: velnor_actions_contract::workflow::TaskBindingDigest,
    input_digest: String,
    closure_digest: String,
    graph_digest: String,
    catalog: &'a ToolCatalog,
    lanes: &'a [NamedCheckLane],
}

fn build_obligation(input: &BuildObligationInput<'_>) -> Result<PlanObligation, OrchestratorError> {
    let task = &input.item.proposal;
    Ok(PlanObligation {
        task_id: task.task_id.clone(),
        decision: ObligationDecision::Execute,
        reason: "opaque_check_requires_execution".to_owned(),
        task_digest: input.task_digest.clone(),
        task_binding_digest: input.task_binding_digest.clone(),
        input_digest: input.input_digest.clone(),
        closure_digest: input.closure_digest.clone(),
        graph_digest: input.graph_digest.clone(),
        mbx_digest: velnor_actions_contract::digest_b3(
            velnor_actions_contract::cachekey::mbx_cache_generation(
                input
                    .catalog
                    .version(velnor_actions_mise::PinnedTool::MrBoxington),
            )
            .as_bytes(),
        ),
        profile: task.configuration.clone(),
        job_id: input
            .lanes
            .first()
            .map(|lane| lane.job_id.clone())
            .ok_or_else(|| internal("named_check_lane_missing"))?,
    })
}

struct NamedCheckEntryContext<'a> {
    task_digest: &'a str,
    input_digest: &'a str,
    run_key: &'a str,
    toolchain_inputs: &'a velnor_actions_contract::cachekey::ToolchainInputs,
    toolchain_id: &'a str,
    logical_platform: &'a PlannedPlatform,
}

fn derive_entries(
    item: &DiscoveredCheck,
    run: &str,
    lanes: &[NamedCheckLane],
    context: &NamedCheckEntryContext<'_>,
) -> Result<Vec<MatrixEntry>, OrchestratorError> {
    let task = &item.proposal;
    let mut entries = Vec::with_capacity(lanes.len());
    for lane in lanes {
        let planned_platform = match lane.runs_on.as_deref() {
            Some(runs_on) => PlannedPlatform::new(runs_on, &context.logical_platform.target)
                .map_err(internal_contract)?,
            None => context.logical_platform.clone(),
        };
        let mut entry = MatrixEntry::derive_for_lane(
            &task.stack_id,
            &task.task_id,
            run,
            context.task_digest,
            super::metadata_for(item).map_err(internal_contract)?,
            execute_ids(task),
            context.input_digest,
            context.run_key,
            &lane.job_id,
            lane.variant,
            planned_platform.clone(),
        )
        .map_err(internal_contract)?;
        entry.declared_outputs = item
            .check
            .evidence
            .iter()
            .map(|evidence| evidence.path.clone())
            .collect();
        entry.toolchain_inputs = Some(context.toolchain_inputs.clone());
        entry.toolchain_id = Some(context.toolchain_id.to_owned());
        entries.push(entry);
    }
    Ok(entries)
}

/// Bind one named check's plan identity to its effective runner label.
///
/// Mise planning ignores the global label: the check's own
/// `runner_profile` selects the runner, so the planned platform must
/// commit to that same label to stay coherent with the group digest.
fn planned_platform_for_check(
    label: &str,
    task: &velnor_actions_contract::ProposedTask,
) -> Result<PlannedPlatform, OrchestratorError> {
    let target = platform_target_for_group(label, task).map_err(internal_contract)?;
    let planned = PlannedPlatform::new(&task.runner_profile, &target).map_err(internal_contract)?;
    if planned.platform_id != platform_id_for_group(label, task).map_err(internal_contract)? {
        return Err(internal("planned_platform_identity_mismatch"));
    }
    Ok(planned)
}
