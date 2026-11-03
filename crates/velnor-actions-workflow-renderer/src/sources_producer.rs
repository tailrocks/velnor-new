// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::workflow::{
    CacheAccess, CacheWriter, SOURCES_PRODUCER_JOB_ID, validate_sources_producer,
};
use velnor_actions_contract::{Job, Step, StepKind, StepRole};

use crate::RenderError;
use crate::render::PLAN_JOB_ID;

const DISPLAY_NAME: &str = "Sources producer";
const HISTORY_INPUT: &str = "fetch-depth";

pub(crate) fn add_sources_producer(jobs: &mut BTreeMap<String, Job>) -> Result<(), RenderError> {
    let Some(plan) = jobs.get(PLAN_JOB_ID) else {
        return Ok(());
    };
    let produces = plan
        .steps
        .iter()
        .any(|step| step.role == Some(StepRole::CargoSourcesRestore));
    if !produces {
        return Ok(());
    }
    let job = build(plan)?;
    validate_sources_producer(&job, SOURCES_PRODUCER_JOB_ID).map_err(RenderError::Contract)?;
    if jobs
        .insert(SOURCES_PRODUCER_JOB_ID.to_owned(), job)
        .is_some()
    {
        return Err(invalid("id_collision"));
    }
    Ok(())
}

fn build(plan: &Job) -> Result<Job, RenderError> {
    let keys_at = plan
        .steps
        .iter()
        .position(|step| step.role == Some(StepRole::CacheKeys))
        .ok_or_else(|| invalid("plan_without_cache_keys"))?;
    let mut steps = Vec::new();
    for step in &plan.steps[..keys_at] {
        steps.push(lean(step.clone()));
    }
    let (restore, save) = crate::plan_sources::source_cache::producer_sources_steps()
        .map_err(|error| invalid(&format!("producer_steps:{error:?}")))?;
    steps.push(restore);
    steps.extend(
        plan.steps[keys_at..]
            .iter()
            .filter(|step| step.role == Some(StepRole::CargoSourcesFetch))
            .cloned(),
    );
    steps.push(save);
    Ok(Job {
        display_name: DISPLAY_NAME.to_owned(),
        runs_on: plan.runs_on.clone(),
        check_runner: None,
        timeout_minutes: crate::cache_save_budget::lift_to_save_budget(plan.timeout_minutes)?,
        needs: vec![PLAN_JOB_ID.to_owned()],
        condition: Some(CacheWriter::SourcesProducer.condition()),
        permissions: None,
        cache_access: CacheAccess::Writer(CacheWriter::SourcesProducer),
        environment: None,
        services: Vec::new(),
        outputs: BTreeMap::new(),
        env: velnor_actions_contract::JobEnv::Unset,
        steps,
    })
}

fn lean(mut step: Step) -> Step {
    if step.role == Some(StepRole::Checkout)
        && let StepKind::Action { with, .. } = &mut step.kind
    {
        with.remove(HISTORY_INPUT);
    }
    step
}

fn invalid(problem: &str) -> RenderError {
    RenderError::InvalidWorkflow(format!("sources_producer:{problem}"))
}
