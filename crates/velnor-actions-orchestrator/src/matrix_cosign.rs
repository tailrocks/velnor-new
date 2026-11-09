// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use velnor_actions_contract::{Step, StepRole, WorkflowPolicy};
use velnor_actions_mise::{PinnedTool, ToolCatalog};

use super::{CrateSuite, LaneTools};
use crate::OrchestratorError;

pub(crate) fn prepare_runtime_cosign_step(
    policy: WorkflowPolicy,
    suite: Option<CrateSuite>,
    catalog: &ToolCatalog,
) -> Result<Option<Step>, OrchestratorError> {
    if policy != WorkflowPolicy::VelnorRepositoryV1 || !suite.is_some_and(CrateSuite::owns_cosign) {
        return Ok(None);
    }
    let mut step = super::prepare_crate_tools_step(
        catalog,
        &LaneTools(BTreeSet::from([PinnedTool::Cosign])),
    )?;
    "Prepare pinned Mac proof verifier".clone_into(&mut step.name);
    step.role = Some(StepRole::PrepareRuntimeTools);
    Ok(Some(step))
}
