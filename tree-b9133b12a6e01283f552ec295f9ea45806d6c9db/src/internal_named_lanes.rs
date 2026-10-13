//! Resolve workflow-emitted named-check lane identities for planning.

use std::collections::BTreeMap;

use velnor_actions_contract::{ExecutionMode, NamedCheckLane, named_check_lanes};

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
    let default =
        named_check_lanes(&prep.workflow.ir, &prep.config, None).map_err(internal_contract)?;
    let Some(supplied) = supplied else {
        return Ok(default);
    };
    for dispatch in candidates {
        let expected = named_check_lanes(&prep.workflow.ir, &prep.config, dispatch)
            .map_err(internal_contract)?;
        if supplied == expected {
            return Ok(supplied);
        }
    }
    Err(internal("named_check_lane_contract_mismatch"))
}
