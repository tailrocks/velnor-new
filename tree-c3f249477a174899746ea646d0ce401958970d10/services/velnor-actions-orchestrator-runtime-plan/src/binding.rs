//! Plan-identity binding for one discovered check.
//!
//! Re-derives the planner's named-check lanes and refuses definition,
//! task, input, or lane drift before execution.

use std::path::Path;

use velnor_actions_contract::canonical_json_bytes;
use velnor_actions_contract_workflow::{
    MatrixEntry, NamedCheckLane, NamedCheckLaneVariant, ObligationDecision, Plan,
};
use velnor_actions_mise::{CheckDeadline, DiscoveredCheck, ToolCatalog};
use velnor_actions_orchestrator_core::OrchestratorError;
use velnor_actions_orchestrator_core::link_safety::reject_link_components;
use velnor_actions_orchestrator_core::{internal, internal_contract};

/// Reuse exactly the planner's derivation, binding definition, task bytes and inputs.
///
/// # Errors
///
/// Returns [`OrchestratorError::Internal`] for task-identity, host-platform,
/// link-escape, lane-derivation, obligation, or digest mismatches.
pub fn bind_check(
    root: &Path,
    item: &DiscoveredCheck,
    plan: &Plan,
    entry: &MatrixEntry,
    task_id: &str,
    catalog: &ToolCatalog,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    if item.proposal.task_id != task_id {
        return Err(internal("check_task_identity"));
    }
    if item.check.runner.platform.os() != std::env::consts::OS
        || item.check.runner.platform.arch() != std::env::consts::ARCH
    {
        return Err(internal("check_host_platform"));
    }
    for input in &item.config_inputs {
        reject_link_components(root, input)?;
    }
    let lane = NamedCheckLane {
        variant: entry.lane_variant,
        job_id: entry.job_id.clone(),
    };
    let argv =
        velnor_actions_orchestrator_provisioning::vectors::task_argv(&item.proposal, catalog)?;
    let (expected, mut expected_entries) =
        velnor_actions_orchestrator_graph::internal_plan::named_checks::plan::derive_lanes_until(
            root,
            item,
            &plan.run_key,
            &plan.generator,
            catalog,
            &[lane],
            Some(deadline),
            &argv,
        )?;
    let expected_entry = expected_entries
        .pop()
        .ok_or_else(|| internal("named_check_lane_missing"))?;
    let actual = plan
        .obligations
        .iter()
        .find(|ob| ob.task_id == task_id)
        .ok_or_else(|| internal("check_obligation_missing"))?;
    if actual.decision != ObligationDecision::Execute
        || actual.task_digest != expected.task_digest
        || actual.input_digest != expected.input_digest
        || actual.closure_digest != expected.closure_digest
        || canonical_json_bytes(entry).map_err(internal_contract)?
            != canonical_json_bytes(&expected_entry).map_err(internal_contract)?
    {
        return Err(internal("check_plan_identity"));
    }
    Ok(())
}

/// Parse the named-check lane vocabulary word.
///
/// # Errors
///
/// Returns [`OrchestratorError::Internal`] for unknown lane words.
pub fn parse_lane_variant(value: &str) -> Result<Option<NamedCheckLaneVariant>, OrchestratorError> {
    match value {
        "single" => Ok(None),
        "hosted" => Ok(Some(NamedCheckLaneVariant::Hosted)),
        "scale_set" => Ok(Some(NamedCheckLaneVariant::ScaleSet)),
        _ => Err(internal("check_lane_variant_invalid")),
    }
}
