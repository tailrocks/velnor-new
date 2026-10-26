// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::{ExecutionMode, NamedCheckLane, expand_workflow, named_check_lanes};

use crate::OrchestratorError;
use crate::internal::{internal, internal_contract};
use crate::prepare::GenerationPreparation;

pub(crate) struct ResolvedLanes {
    pub(crate) named_check_lanes: BTreeMap<String, Vec<NamedCheckLane>>,
    pub(crate) bun_job_lanes: BTreeMap<String, Vec<String>>,
}

pub(crate) fn resolve(
    prep: &GenerationPreparation,
    supplied_named: Option<&BTreeMap<String, Vec<NamedCheckLane>>>,
    supplied_bun: Option<&BTreeMap<String, Vec<String>>>,
) -> Result<ResolvedLanes, OrchestratorError> {
    let candidates = [
        None,
        Some(ExecutionMode::Hosted),
        Some(ExecutionMode::ScaleSet),
        Some(ExecutionMode::Both),
    ];
    for dispatch in candidates {
        let expansion = match expand_workflow(&prep.workflow.ir, &prep.config, dispatch) {
            Ok(expansion) => expansion,
            Err(error) if dispatch.is_none() => return Err(internal_contract(error)),
            Err(_) => continue,
        };
        let expected_named = match named_check_lanes(&prep.workflow.ir, &prep.config, dispatch) {
            Ok(lanes) => lanes,
            Err(error) if dispatch.is_none() => return Err(internal_contract(error)),
            Err(_) => continue,
        };
        if supplied_named.is_some_and(|supplied| supplied != &expected_named)
            || supplied_bun.is_some_and(|supplied| supplied != &expansion.bun_job_lanes)
        {
            continue;
        }
        return Ok(ResolvedLanes {
            named_check_lanes: expected_named,
            bun_job_lanes: expansion.bun_job_lanes,
        });
    }
    let problem = match (supplied_named.is_some(), supplied_bun.is_some()) {
        (true, false) => "named_check_lane_contract_mismatch",
        (false, true) => "bun_job_lane_contract_mismatch",
        (true, true) | (false, false) => "planning_lane_contract_mismatch",
    };
    Err(internal(problem))
}
