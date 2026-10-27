// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fs;

use velnor_actions_contract::REQUIRED_JOB_ID;
use velnor_actions_orchestrator::{GenerationPreparation, prepare, render_staged_tree};
use velnor_actions_workflow_renderer::render::WORKFLOW_PATH;

use super::support::{config_with_timeouts, rendered_job_block, rust_owner_jobs};
use super::workflow_configuration_fixture;

#[test]
fn prepare_and_render_keep_plan_and_crate_timeouts_independent() -> Result<(), Box<dyn Error>> {
    let initial_config = config_with_timeouts(15, Some(7))?;
    let repo = workflow_configuration_fixture::make_repo(&initial_config)?;
    let root = repo.path();
    let expanded_plan = prepare(root)?;
    let expanded_rendered = render_staged_tree(&expanded_plan)?;

    fs::write(
        root.join(".velnor/config.toml"),
        config_with_timeouts(5, Some(7))?,
    )?;
    let reduced_plan = prepare(root)?;
    let reduced_rendered = render_staged_tree(&reduced_plan)?;

    assert_ir_timeouts_are_independent(&expanded_plan, &reduced_plan);
    let expanded_yaml = expanded_rendered
        .get(WORKFLOW_PATH)
        .ok_or("15-minute workflow is missing")?;
    let reduced_yaml = reduced_rendered
        .get(WORKFLOW_PATH)
        .ok_or("5-minute workflow is missing")?;
    assert_rendered_timeouts_are_independent(expanded_yaml, reduced_yaml, &expanded_plan)?;
    Ok(())
}

fn assert_ir_timeouts_are_independent(
    expanded_plan: &GenerationPreparation,
    reduced_plan: &GenerationPreparation,
) {
    let plan_id = velnor_actions_workflow_renderer::render::PLAN_JOB_ID;
    assert_eq!(
        expanded_plan.workflow.ir.jobs[plan_id]
            .timeout_minutes
            .minutes(),
        15
    );
    assert_eq!(
        reduced_plan.workflow.ir.jobs[plan_id]
            .timeout_minutes
            .minutes(),
        5
    );
    let expanded_crate_jobs = rust_owner_jobs(expanded_plan);
    let reduced_crate_jobs = rust_owner_jobs(reduced_plan);
    assert!(!expanded_crate_jobs.is_empty());
    assert_eq!(expanded_crate_jobs.len(), reduced_crate_jobs.len());
    assert_eq!(
        expanded_crate_jobs
            .iter()
            .map(|(id, _)| *id)
            .collect::<BTreeSet<_>>(),
        reduced_crate_jobs
            .iter()
            .map(|(id, _)| *id)
            .collect::<BTreeSet<_>>()
    );
    for (id, job) in expanded_crate_jobs {
        assert_eq!(job.timeout_minutes.minutes(), 7, "{id}");
        assert_eq!(
            reduced_plan.workflow.ir.jobs[id].timeout_minutes.minutes(),
            7,
            "{id}"
        );
    }
    assert_eq!(
        expanded_plan.workflow.ir.jobs[REQUIRED_JOB_ID]
            .timeout_minutes
            .minutes(),
        5
    );
    assert_eq!(
        reduced_plan.workflow.ir.jobs[REQUIRED_JOB_ID]
            .timeout_minutes
            .minutes(),
        5
    );
    assert_eq!(
        timeouts_without_plan(expanded_plan),
        timeouts_without_plan(reduced_plan)
    );
}

fn assert_rendered_timeouts_are_independent(
    expanded_yaml: &str,
    reduced_yaml: &str,
    expanded_plan: &GenerationPreparation,
) -> Result<(), Box<dyn Error>> {
    let plan_id = velnor_actions_workflow_renderer::render::PLAN_JOB_ID;
    let expanded_plan_yaml = rendered_job_block(expanded_yaml, plan_id)?;
    let reduced_plan_yaml = rendered_job_block(reduced_yaml, plan_id)?;
    let expanded_plan_timeout = "    timeout-minutes: 15\n";
    let reduced_plan_timeout = "    timeout-minutes: 5\n";
    let expanded_cache_seconds = "VELNOR_CACHE_JOB_SECONDS: \"900\"";
    let reduced_cache_seconds = "VELNOR_CACHE_JOB_SECONDS: \"300\"";
    assert_eq!(expanded_plan_yaml.matches(expanded_plan_timeout).count(), 1);
    assert_eq!(reduced_plan_yaml.matches(reduced_plan_timeout).count(), 1);
    assert!(expanded_plan_yaml.contains(expanded_cache_seconds));
    assert!(reduced_plan_yaml.contains(reduced_cache_seconds));
    assert_eq!(
        expanded_plan_yaml.matches(expanded_cache_seconds).count(),
        reduced_plan_yaml.matches(reduced_cache_seconds).count()
    );
    assert_eq!(
        expanded_plan_yaml
            .replacen(expanded_plan_timeout, reduced_plan_timeout, 1)
            .replace(expanded_cache_seconds, reduced_cache_seconds),
        reduced_plan_yaml
    );
    for job_id in expanded_plan
        .workflow
        .ir
        .jobs
        .keys()
        .filter(|job_id| job_id.as_str() != plan_id)
    {
        assert_eq!(
            rendered_job_block(expanded_yaml, job_id)?,
            rendered_job_block(reduced_yaml, job_id)?,
            "non-Plan rendered job {job_id}"
        );
    }
    Ok(())
}

fn timeouts_without_plan(prepared: &GenerationPreparation) -> BTreeMap<String, u16> {
    prepared
        .workflow
        .ir
        .jobs
        .iter()
        .filter(|(id, _)| id.as_str() != velnor_actions_workflow_renderer::render::PLAN_JOB_ID)
        .map(|(id, job)| (id.clone(), job.timeout_minutes.minutes()))
        .collect()
}
