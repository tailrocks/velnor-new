// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::{Job, Step, StepKind, StepRole};

use crate::{RenderError, render::PLAN_JOB_ID, steps};

pub const FORMAT_STEP_NAME: &str = "Format";

pub fn format_step(argv: Vec<String>, env: &BTreeMap<String, String>) -> Result<Step, RenderError> {
    if argv.first().is_none_or(|program| program != "mise") {
        return Err(RenderError::BadCommand("format_without_mise".to_owned()));
    }
    let mut step = steps::shell_step(FORMAT_STEP_NAME, argv, env.clone())?;
    step.role = Some(StepRole::PlanFormat);
    Ok(step)
}

pub fn ensure_plan_format(
    jobs: &mut BTreeMap<String, Job>,
    argv: Vec<String>,
    env: &BTreeMap<String, String>,
) -> Result<(), RenderError> {
    let Some(plan) = jobs.get_mut(PLAN_JOB_ID) else {
        return Ok(());
    };
    if let Some(format) = plan
        .steps
        .iter()
        .find(|step| step.role == Some(StepRole::PlanFormat))
    {
        return check_format_shape(format);
    }
    let step = format_step(argv, env)?;
    let at = format_insert_at(plan);
    plan.steps.insert(at, step);
    Ok(())
}

fn check_format_shape(format: &Step) -> Result<(), RenderError> {
    if matches!(format.kind, StepKind::Shell { .. }) {
        Ok(())
    } else {
        Err(RenderError::InvalidWorkflow(
            "format_step_malformed".to_owned(),
        ))
    }
}

fn format_insert_at(plan: &Job) -> usize {
    plan.steps
        .iter()
        .position(|step| step.role == Some(StepRole::CheckGenerated))
        .or_else(|| {
            plan.steps
                .iter()
                .position(|step| step.role == Some(StepRole::PlanProducer))
        })
        .or_else(|| {
            plan.steps
                .iter()
                .position(|step| step.role == Some(StepRole::PublishPlan))
        })
        .unwrap_or(plan.steps.len())
}
