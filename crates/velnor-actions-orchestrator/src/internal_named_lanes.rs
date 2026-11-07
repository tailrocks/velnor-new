// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::{ExecutionMode, NamedCheckLane, named_check_lanes};

use crate::OrchestratorError;
use crate::internal::{internal, internal_contract};
use crate::planner::prepare::GenerationPreparation;

pub(crate) fn resolve_routes(
    prep: &GenerationPreparation,
    supplied: Option<&velnor_actions_contract::LaneInventory>,
    selected_checks: &BTreeMap<String, Vec<NamedCheckLane>>,
) -> Result<Option<velnor_actions_contract::LaneInventory>, OrchestratorError> {
    let routes = |dispatch| {
        velnor_actions_contract::expand_workflow_with_lane_plan(
            &prep.workflow.ir,
            &prep.config,
            dispatch,
        )
        .map(|expansion| expansion.lane_plan.map(|plan| plan.inventory()))
        .map_err(internal_contract)
    };
    for dispatch in [
        None,
        Some(ExecutionMode::Hosted),
        Some(ExecutionMode::ScaleSet),
        Some(ExecutionMode::Both),
    ] {
        let checks = named_check_lanes(&prep.workflow.ir, &prep.config, dispatch)
            .map_err(internal_contract)?;
        if checks != *selected_checks {
            continue;
        }
        let expected = routes(dispatch)?;
        if supplied.is_none_or(|value| Some(value) == expected.as_ref()) {
            return Ok(expected.filter(|inventory| {
                (supplied.is_some()
                    && inventory.mode
                        != prep
                            .config
                            .execution
                            .as_ref()
                            .and_then(|execution| execution.mode))
                    || inventory.routes.iter().any(|route| {
                        route.variant.is_some()
                            || matches!(
                                velnor_actions_contract::config::RunsOn::parse(&route.runner),
                                Ok(velnor_actions_contract::config::RunsOn::ScaleSet(_))
                            )
                    })
            }));
        }
    }
    Err(internal("lane_route_contract_mismatch"))
}

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
