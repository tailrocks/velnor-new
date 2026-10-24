//! Independent named check jobs, with plan-bound reports and typed runner admission.

use std::collections::BTreeMap;

use velnor_actions_contract::config::{CheckExecutor, EPHEMERAL_CHECK_ADMISSION_CONDITION};
use velnor_actions_contract::{
    Job, JobTimeout, NAMED_CHECK_JOB_ID_ENV, NAMED_CHECK_LANE_VARIANT_ENV, Permissions, Step,
    WorkflowPolicy,
};
use velnor_actions_mise::{DiscoveredCheck, ToolCatalog};
use velnor_actions_workflow_renderer::render::PLAN_JOB_ID;
use velnor_actions_workflow_renderer::steps::INTERNAL_OP_ENV;

use crate::OrchestratorError;
use crate::discover::Discovery;

/// Build one job per configured check, independent of Rust grouping and coverage.
#[cfg(test)]
pub(crate) fn build_check_jobs(
    policy: WorkflowPolicy,
    discovery: &Discovery,
    catalog: &ToolCatalog,
) -> Result<Vec<(String, Job)>, OrchestratorError> {
    build_check_jobs_with_qualification(policy, discovery, catalog, false)
}

/// Make one check job for both plan modes when Qualification is configured.
pub(crate) fn build_check_jobs_with_qualification(
    policy: WorkflowPolicy,
    discovery: &Discovery,
    catalog: &ToolCatalog,
    qualification_enabled: bool,
) -> Result<Vec<(String, Job)>, OrchestratorError> {
    let mut jobs = Vec::with_capacity(discovery.mise_checks.len());
    for discovered in &discovery.mise_checks {
        let check = &discovered.check;
        let acquire = match policy {
            WorkflowPolicy::ConsumerV1 => Some(crate::pins::consumer_acquire_for_runner(
                &check.runner,
                env!("CARGO_PKG_VERSION"),
                discovery.consumer_manifest_json.as_deref(),
            )?),
            WorkflowPolicy::VelnorRepositoryV1 => None,
        };
        let id = format!("check-{}", check.id);
        jobs.push((
            id.clone(),
            check_job(discovered, &id, acquire, catalog, qualification_enabled)?,
        ));
    }
    jobs.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(jobs)
}

fn check_job(
    discovered: &DiscoveredCheck,
    job_id: &str,
    acquire: Option<Step>,
    catalog: &ToolCatalog,
    qualification_enabled: bool,
) -> Result<Job, OrchestratorError> {
    let check = &discovered.check;
    let mut steps = vec![crate::workflow::wire_w1::checkout_step_full()?];
    steps.extend(acquire);
    steps.push(crate::matrix_step::download_plan_step()?);
    steps.push(execute_check_step(discovered, job_id, catalog)?);
    if qualification_enabled {
        let mut evidence = crate::qualification_source_evidence::source_evidence_step(catalog)?;
        evidence.condition = Some("needs.plan.outputs.qualification_campaign != ''".to_owned());
        steps.push(evidence);
    }
    let mut upload = crate::matrix_step::crate_upload_step(job_id)?;
    upload.condition = Some("always()".to_owned());
    steps.push(upload);
    let minutes =
        u16::try_from(check.timeout_minutes).map_err(|_| OrchestratorError::Contract {
            problem: format!("check_timeout_out_of_range:{}", check.id),
        })?;
    Ok(Job {
        services: Vec::new(),
        display_name: format!("Check / {}", check.id),
        runs_on: check.runner.label.clone(),
        check_runner: Some(check.runner.clone()),
        timeout_minutes: JobTimeout::new(minutes)?,
        needs: vec![PLAN_JOB_ID.to_owned()],
        condition: check_condition(check.runner.executor, qualification_enabled),
        permissions: Some(Permissions::default()),
        environment: None,
        steps,
        cache_mode: None,
    })
}

/// Use one check job in both plan modes with the same runner admission checks.
fn check_condition(executor: CheckExecutor, qualification_enabled: bool) -> Option<String> {
    if !qualification_enabled {
        return (executor == CheckExecutor::EphemeralSelfHosted)
            .then(|| EPHEMERAL_CHECK_ADMISSION_CONDITION.to_owned());
    }
    let condition = if executor == CheckExecutor::EphemeralSelfHosted {
        format!(
            "always() && needs.plan.result == 'success' && ((needs.plan.outputs.qualification_campaign == '' && ({EPHEMERAL_CHECK_ADMISSION_CONDITION})) || needs.plan.outputs.qualification_campaign != '')"
        )
    } else {
        "always() && needs.plan.result == 'success'".to_owned()
    };
    Some(condition)
}

/// Only the qualified helper executes repository task code and writes evidence.
/// The check stays unconditional: opaque checks cannot inherit Rust reuse.
fn execute_check_step(
    discovered: &DiscoveredCheck,
    job_id: &str,
    catalog: &ToolCatalog,
) -> Result<Step, OrchestratorError> {
    let identity = BTreeMap::from([
        ("VELNOR_CHECK_ID".to_owned(), discovered.check.id.clone()),
        (NAMED_CHECK_JOB_ID_ENV.to_owned(), job_id.to_owned()),
        (NAMED_CHECK_LANE_VARIANT_ENV.to_owned(), "single".to_owned()),
        (
            "VELNOR_TASK_ID".to_owned(),
            discovered.proposal.task_id.clone(),
        ),
        (INTERNAL_OP_ENV.to_owned(), "execute-check-v1".to_owned()),
    ]);
    let env = crate::matrix_step::task_step_env(catalog, &identity, false)?;
    velnor_actions_workflow_renderer::shell_step(
        "Execute named check",
        vec![crate::matrix_step::helper_path_for_version()],
        env,
    )
    .map_err(OrchestratorError::from)
}

#[cfg(test)]
#[path = "check_jobs_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "check_jobs_package_tests.rs"]
mod package_tests;

#[cfg(test)]
#[path = "check_jobs_maintenance_tests.rs"]
mod maintenance_tests;
