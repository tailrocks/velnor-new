// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::config::{
    CheckExecutor, EPHEMERAL_CHECK_ADMISSION_CONDITION, NativeHomebrewCheck,
};
use velnor_actions_contract::{
    Job, JobTimeout, NAMED_CHECK_JOB_ID_ENV, NAMED_CHECK_LANE_VARIANT_ENV, Permissions, Step,
    WorkflowPolicy,
};
use velnor_actions_mise::{DiscoveredCheck, ToolCatalog};
use velnor_actions_workflow_renderer::render::PLAN_JOB_ID;
use velnor_actions_workflow_renderer::steps::INTERNAL_OP_ENV;

use crate::OrchestratorError;
use crate::discover::Discovery;

pub(crate) fn build_check_jobs(
    policy: WorkflowPolicy,
    discovery: &Discovery,
    catalog: &ToolCatalog,
    published_lock: Option<&velnor_actions_contract::GeneratorLock>,
    helper_version: &str,
) -> Result<Vec<(String, Job)>, OrchestratorError> {
    let mut jobs = Vec::with_capacity(discovery.mise_checks.len() + discovery.native_checks.len());
    for discovered in &discovery.mise_checks {
        let check = &discovered.check;
        let acquire = match policy {
            WorkflowPolicy::ConsumerV1 => Some(crate::pins::consumer_acquire_for_runner(
                &check.runner,
                env!("CARGO_PKG_VERSION"),
                discovery.consumer_manifest_json.as_deref(),
            )?),
            WorkflowPolicy::VelnorRepositoryV1 => Some(published_lock_acquire_for_runner(
                published_lock,
                &check.runner,
                helper_version,
            )?),
        };
        let id = format!("check-{}", check.id);
        jobs.push((
            id.clone(),
            check_job(discovered, &id, acquire, catalog, helper_version)?,
        ));
    }
    for check in &discovery.native_checks {
        let id = format!("check-{}", check.id);
        let acquire = match policy {
            WorkflowPolicy::ConsumerV1 => Some(crate::pins::consumer_acquire_for_runner(
                &check.runner,
                env!("CARGO_PKG_VERSION"),
                discovery.consumer_manifest_json.as_deref(),
            )?),
            WorkflowPolicy::VelnorRepositoryV1 => Some(published_lock_acquire_for_runner(
                published_lock,
                &check.runner,
                helper_version,
            )?),
        };
        jobs.push((
            id.clone(),
            native_check_job(check, &id, acquire, catalog, helper_version)?,
        ));
    }
    jobs.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(jobs)
}

fn native_check_job(
    check: &NativeHomebrewCheck,
    job_id: &str,
    acquire: Option<Step>,
    catalog: &ToolCatalog,
    helper_version: &str,
) -> Result<Job, OrchestratorError> {
    let task_id = format!("internal/native-check/{}", check.id);
    let step = Step {
        name: "Execute native Homebrew check".to_owned(),
        id: None,
        role: None,
        condition: None,
        parallel: false,
        kind: velnor_actions_contract::StepKind::NativeCheckExecution {
            check_id: check.id.clone(),
            operation: check.operation,
            task_id,
            sdk_version: helper_version.to_owned(),
        },
    };
    let prepare_gh = crate::workflow_jobs::prepare_pinned_tools_step(
        catalog,
        vec![velnor_actions_mise::PinnedTool::Gh],
        false,
    )?;
    let mut steps = vec![crate::workflow::wire_w1::checkout_step_full()?];
    steps.extend(acquire);
    steps.push(crate::matrix_step::download_plan_step()?);
    steps.push(prepare_gh);
    steps.push(step);
    let mut upload = crate::matrix_step::crate_upload_step(job_id)?;
    upload.condition = Some("always()".to_owned());
    steps.push(upload);
    let minutes =
        u16::try_from(check.timeout_minutes).map_err(|_| OrchestratorError::Contract {
            problem: format!("check_timeout_out_of_range:{}", check.id),
        })?;
    Ok(Job {
        services: Vec::new(),
        display_name: format!("Homebrew / {}", check.id),
        runs_on: check.runner.label.clone(),
        check_runner: Some(check.runner.clone()),
        timeout_minutes: JobTimeout::new(minutes)?,
        needs: vec![PLAN_JOB_ID.to_owned()],
        condition: None,
        permissions: Some(crate::workflow_jobs::read_actions_permissions()),
        environment: None,
        steps,
        cache_mode: None,
    })
}

fn check_job(
    discovered: &DiscoveredCheck,
    job_id: &str,
    acquire: Option<Step>,
    catalog: &ToolCatalog,
    helper_version: &str,
) -> Result<Job, OrchestratorError> {
    let check = &discovered.check;
    let mut steps = vec![crate::workflow::wire_w1::checkout_step_full()?];
    steps.extend(acquire);
    steps.push(crate::matrix_step::download_plan_step()?);
    steps.push(execute_check_step(discovered, catalog, helper_version)?);
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
        condition: (check.runner.executor == CheckExecutor::EphemeralSelfHosted)
            .then(|| EPHEMERAL_CHECK_ADMISSION_CONDITION.to_owned()),
        permissions: Some(Permissions::default()),
        environment: None,
        steps,
        cache_mode: None,
    })
}

fn published_lock_acquire_for_runner(
    lock: Option<&velnor_actions_contract::GeneratorLock>,
    runner: &velnor_actions_contract::config::CheckRunner,
    helper_version: &str,
) -> Result<Step, OrchestratorError> {
    let lock = lock.ok_or_else(|| OrchestratorError::Contract {
        problem: "required_verdict_requires_published_generator_lock".to_owned(),
    })?;
    crate::attach::published_lock_acquire_for_runner(lock, runner, helper_version)
}

fn execute_check_step(
    discovered: &DiscoveredCheck,
    catalog: &ToolCatalog,
    helper_version: &str,
) -> Result<Step, OrchestratorError> {
    let identity = BTreeMap::from([
        ("VELNOR_CHECK_ID".to_owned(), discovered.check.id.clone()),
        (
            NAMED_CHECK_JOB_ID_ENV.to_owned(),
            format!("check-{}", discovered.check.id),
        ),
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
        vec![crate::matrix_step::helper_path_for_version(helper_version)],
        env,
    )
    .map_err(OrchestratorError::from)
}
