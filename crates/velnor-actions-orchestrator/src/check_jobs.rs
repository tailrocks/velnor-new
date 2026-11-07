// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::config::{
    CheckExecutor, EPHEMERAL_CHECK_ADMISSION_CONDITION, NativeHomebrewCheck,
};
use velnor_actions_contract::{
    CheckoutHistory, Job, JobTimeout, NAMED_CHECK_JOB_ID_ENV, NAMED_CHECK_LANE_VARIANT_ENV,
    Permissions, RunsOn, Step, WorkflowPolicy,
};
use velnor_actions_mise::{DiscoveredCheck, ToolCatalog};
use velnor_actions_workflow_renderer::render::PLAN_JOB_ID;
use velnor_actions_workflow_renderer::steps::INTERNAL_OP_ENV;

use crate::OrchestratorError;
use crate::planner::discover::Discovery;

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
            WorkflowPolicy::ConsumerV1 => Some(crate::planner::pins::consumer_acquire_for_runner(
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
            check_job(
                discovered,
                &id,
                acquire,
                catalog,
                helper_version,
                published_lock.is_some_and(|lock| lock.source_preview.is_some()),
            )?,
        ));
    }
    for check in &discovery.native_checks {
        let id = format!("check-{}", check.id);
        let acquire = match policy {
            WorkflowPolicy::ConsumerV1 => Some(crate::planner::pins::consumer_acquire_for_runner(
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

pub(crate) fn require_history_push_branches(
    discovery: &Discovery,
    push_branches: &[String],
) -> Result<(), OrchestratorError> {
    for discovered in &discovery.mise_checks {
        let check = &discovered.check;
        if !check.history.permits_push_branches(push_branches) {
            return Err(OrchestratorError::config(
                crate::config::CONFIG_REL,
                format!("checks.{}.history", check.id),
                "shallow_on_push_requires_push_branch_main",
            ));
        }
    }
    Ok(())
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
    let mut steps = vec![crate::workflow::wire_w1::checkout_step_for(
        CheckoutHistory::Full,
    )?];
    steps.extend(acquire);
    steps.push(crate::planner::matrix_step::download_plan_step()?);
    steps.push(prepare_gh);
    steps.push(step);
    let mut upload = crate::planner::matrix_step::crate_upload_step(job_id)?;
    upload.condition = Some("always()".to_owned());
    steps.push(upload);
    let minutes =
        u16::try_from(check.timeout_minutes).map_err(|_| OrchestratorError::Contract {
            problem: format!("check_timeout_out_of_range:{}", check.id),
        })?;
    Ok(Job {
        services: Vec::new(),
        outputs: BTreeMap::new(),
        display_name: format!("Homebrew / {}", check.id),
        runs_on: check.runner.label.clone(),
        check_runner: Some(check.runner.clone()),
        timeout_minutes: JobTimeout::new(minutes)?,
        needs: vec![PLAN_JOB_ID.to_owned()],
        condition: None,
        permissions: Some(crate::workflow_jobs::read_actions_permissions()),
        environment: None,
        env: velnor_actions_contract::JobEnv::Unset,
        steps,
        cache_access: velnor_actions_contract::CacheAccess::Read,
    })
}

fn check_job(
    discovered: &DiscoveredCheck,
    job_id: &str,
    acquire: Option<Step>,
    catalog: &ToolCatalog,
    helper_version: &str,
    preview: bool,
) -> Result<Job, OrchestratorError> {
    let check = &discovered.check;
    let mut checkout = crate::workflow::wire_w1::checkout_step_for(check.history)?;
    if check.checkout_blob_closure {
        let velnor_actions_contract::StepKind::Action { with, .. } = &mut checkout.kind else {
            return Err(OrchestratorError::Contract {
                problem: "check checkout is not an action".to_owned(),
            });
        };
        with.insert(
            "filter".to_owned(),
            velnor_actions_contract::workflow::step_identity::FIXTURE_CHECKOUT_BLOB_FILTER
                .to_owned(),
        );
    }
    let mut steps = vec![checkout];
    steps.extend(acquire);
    steps.push(crate::planner::matrix_step::download_plan_step()?);
    steps.push(execute_check_step(
        discovered,
        catalog,
        helper_version,
        preview,
    )?);
    let mut upload = crate::planner::matrix_step::crate_upload_step(job_id)?;
    upload.condition = Some("always()".to_owned());
    steps.push(upload);
    let minutes =
        u16::try_from(check.timeout_minutes).map_err(|_| OrchestratorError::Contract {
            problem: format!("check_timeout_out_of_range:{}", check.id),
        })?;
    Ok(Job {
        services: Vec::new(),
        outputs: BTreeMap::new(),
        display_name: format!("Check / {}", check.id),
        runs_on: check.runner.label.clone(),
        check_runner: Some(check.runner.clone()),
        timeout_minutes: JobTimeout::new(minutes)?,
        needs: vec![PLAN_JOB_ID.to_owned()],
        condition: Some(named_check_condition(job_id, check)),
        permissions: Some(Permissions::default()),
        environment: None,
        env: velnor_actions_contract::JobEnv::Unset,
        steps,
        cache_access: velnor_actions_contract::CacheAccess::Read,
    })
}

fn named_check_condition(
    job_id: &str,
    check: &velnor_actions_contract::config::MiseCheck,
) -> String {
    let gate = velnor_actions_workflow_renderer::not_affected_gate(job_id);
    let needs_admission = check.runner.executor == CheckExecutor::EphemeralSelfHosted
        || RunsOn::parse(&check.runner.label).is_ok_and(|selector| selector.is_scale_set());
    if needs_admission {
        format!("({gate}) && ({EPHEMERAL_CHECK_ADMISSION_CONDITION})")
    } else {
        gate
    }
}

fn published_lock_acquire_for_runner(
    lock: Option<&velnor_actions_contract::GeneratorLock>,
    runner: &velnor_actions_contract::config::CheckRunner,
    helper_version: &str,
) -> Result<Step, OrchestratorError> {
    let lock = lock.ok_or_else(|| OrchestratorError::Contract {
        problem: "required_verdict_requires_published_generator_lock".to_owned(),
    })?;
    crate::planner::attach::published_lock_acquire_for_runner(lock, runner, helper_version)
}

fn execute_check_step(
    discovered: &DiscoveredCheck,
    catalog: &ToolCatalog,
    helper_version: &str,
    preview: bool,
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
    let mut env = crate::planner::matrix_step::task_step_env(catalog, &identity, false)?;
    if preview {
        env.remove(INTERNAL_OP_ENV);
        return Ok(Step {
            parallel: false,
            name: "Execute named check".to_owned(),
            id: None,
            role: None,
            condition: None,
            kind: velnor_actions_contract::StepKind::Internal {
                operation: "execute-check-v1".to_owned(),
                env,
            },
        });
    }
    velnor_actions_workflow_renderer::shell_step(
        "Execute named check",
        vec![crate::planner::matrix_step::helper_path_for_version(
            helper_version,
        )],
        env,
    )
    .map_err(OrchestratorError::from)
}
