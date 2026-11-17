// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{HelperCommand, RepoPath};

use crate::base_steps::base_age;
use crate::gate::{PLAN_STEP, REPLAN_STEP};
use crate::release_steps::shell_word;
use crate::rules::RenderedStep;
use crate::steps::PREPARE_STEP;

pub(crate) fn inputs_step(
    verb: &str,
    target: &RepoPath,
    directory: Option<&RepoPath>,
) -> RenderedStep {
    let depth = directory.map_or(0, |dir| dir.as_str().split('/').count());
    let relative = format!("{}{}", "../".repeat(depth), target.as_str());
    RenderedStep {
        name: format!("velnor-ci inputs {verb}"),
        run: Some(format!(
            "velnor-ci inputs {verb} --target-dir {}",
            shell_word(&relative)
        )),
        ..RenderedStep::default()
    }
}

pub(crate) fn helper_step(command: HelperCommand) -> RenderedStep {
    let arguments = match command {
        HelperCommand::Plan => "plan",
        HelperCommand::PlanFull => "plan --full",
        HelperCommand::Prepare => "prepare",
        HelperCommand::CheckMetadata => "check-metadata",
        HelperCommand::Restored(..) => "restored",
        HelperCommand::InputsAudit => "inputs audit",
    };
    let (id, env) = match command {
        HelperCommand::Plan => (Some(PLAN_STEP), Vec::new()),
        HelperCommand::PlanFull => (Some(REPLAN_STEP), Vec::new()),
        HelperCommand::Prepare => (Some(PREPARE_STEP), Vec::new()),
        HelperCommand::Restored(expiry) => (Some("restored"), base_age(expiry)),
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

pub(crate) fn restored_step(
    expiry: velnor_model::BaseExpiry,
    targets: &[RepoPath],
) -> RenderedStep {
    let mut step = helper_step(HelperCommand::Restored(expiry));
    step.env.push((
        "VELNOR_CACHE_TARGETS".to_owned(),
        crate::rules::Value::Text(
            targets
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("\n"),
        ),
    ));
    step
}
