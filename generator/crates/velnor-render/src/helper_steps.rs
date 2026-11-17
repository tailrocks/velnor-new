// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::HelperCommand;

use crate::base_steps::base_age;
use crate::gate::{PLAN_STEP, REPLAN_STEP};
use crate::rules::RenderedStep;
use crate::steps::PREPARE_STEP;

const RESTORED_STEP: &str = "restored";

fn helper_arguments(command: HelperCommand) -> &'static str {
    match command {
        HelperCommand::Plan => "plan",
        HelperCommand::PlanFull => "plan --full",
        HelperCommand::Prepare => "prepare",
        HelperCommand::CheckMetadata => "check-metadata",
        HelperCommand::Restored(..) => "restored",
        HelperCommand::InputsSave => "inputs save",
        HelperCommand::InputsRestore => "inputs restore",
        HelperCommand::InputsAudit => "inputs audit",
    }
}

pub(crate) fn helper_step(command: HelperCommand) -> RenderedStep {
    let arguments = helper_arguments(command);
    let (id, env) = match command {
        HelperCommand::Plan => (Some(PLAN_STEP), Vec::new()),
        HelperCommand::PlanFull => (Some(REPLAN_STEP), Vec::new()),
        HelperCommand::Prepare => (Some(PREPARE_STEP), Vec::new()),
        HelperCommand::Restored(expiry) => (Some(RESTORED_STEP), base_age(expiry)),
        _ => (None, Vec::new()),
    };
    RenderedStep {
        name: format!("velnor-ci {arguments}"),
        id: id.map(str::to_owned),
        env,
        run: Some(match command {
            HelperCommand::Plan => format!("velnor-ci {arguments} --explain"),
            _ => format!("velnor-ci {arguments}"),
        }),
        ..RenderedStep::default()
    }
}
