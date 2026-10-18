//! Resolve workflow-emitted named-check lane identities for planning.

use std::collections::BTreeMap;

use velnor_actions_contract::{
    ExecutionMode, NamedCheckLane, named_check_lanes, qualification_job_lanes,
};

use crate::OrchestratorError;
use crate::internal::{internal, internal_contract};
use crate::prepare::GenerationPreparation;

/// Verify the request's lane map against all supported workflow dispatch modes.
pub(crate) fn resolve(
    prep: &GenerationPreparation,
    supplied: Option<BTreeMap<String, Vec<NamedCheckLane>>>,
) -> Result<BTreeMap<String, Vec<NamedCheckLane>>, OrchestratorError> {
    let candidates = [
        None,
        Some(ExecutionMode::Hosted),
        Some(ExecutionMode::ScaleSet),
        Some(ExecutionMode::Both),
    ];
    let default = lanes_for_dispatch(prep, None)?;
    let Some(supplied) = supplied else {
        return Ok(default);
    };
    for dispatch in candidates {
        let expected = lanes_for_dispatch(prep, dispatch)?;
        if supplied == expected {
            return Ok(supplied);
        }
    }
    Err(internal("named_check_lane_contract_mismatch"))
}

fn lanes_for_dispatch(
    prep: &GenerationPreparation,
    dispatch: Option<ExecutionMode>,
) -> Result<BTreeMap<String, Vec<NamedCheckLane>>, OrchestratorError> {
    let mut lanes =
        named_check_lanes(&prep.workflow.ir, &prep.config, dispatch).map_err(internal_contract)?;
    let sources = crate::crate_job_ids::qualification_job_sources_for_ir(
        &prep.discovery.proposals,
        &prep.workflow.ir,
    )
    .map_err(internal_contract)?;
    for (id, entries) in
        qualification_job_lanes(&prep.workflow.ir, &prep.config, dispatch, &sources)
            .map_err(internal_contract)?
    {
        if lanes.insert(id, entries).is_some() {
            return Err(internal("executor_lane_map_collision"));
        }
    }
    Ok(lanes)
}
