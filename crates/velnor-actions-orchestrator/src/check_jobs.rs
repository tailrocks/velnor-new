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
use crate::discover::Discovery;

#[cfg(all(test, unix))]
#[path = "check_jobs_tests.rs"]
mod tests;

pub(crate) fn build_check_jobs(
    policy: WorkflowPolicy,
    config: &velnor_actions_contract::VelnorConfig,
    discovery: &Discovery,
    catalog: &ToolCatalog,
    published_lock: Option<&velnor_actions_contract::GeneratorLock>,
    approved_release: Option<velnor_actions_mise::catalog::lock::ApprovedGeneratorRelease<'_>>,
    helper_version: &str,
) -> Result<Vec<(String, Job)>, OrchestratorError> {
    let lane = crate::matrix_step::LaneTools::union(
        policy,
        catalog,
        &crate::crate_job_ids::group_runnable(&discovery.proposals)?,
    );
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
            check_job(
                discovered,
                &id,
                acquire,
                catalog,
                helper_version,
                &lane,
                CheckContext {
                    policy,
                    config,
                    approved_release,
                },
            )?,
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

#[derive(Clone, Copy)]
struct CheckContext<'a> {
    policy: WorkflowPolicy,
    config: &'a velnor_actions_contract::VelnorConfig,
    approved_release: Option<velnor_actions_mise::catalog::lock::ApprovedGeneratorRelease<'a>>,
}

fn check_job(
    discovered: &DiscoveredCheck,
    job_id: &str,
    acquire: Option<Step>,
    catalog: &ToolCatalog,
    helper_version: &str,
    lane: &crate::matrix_step::LaneTools,
    context: CheckContext<'_>,
) -> Result<Job, OrchestratorError> {
    let CheckContext {
        policy,
        config,
        approved_release,
    } = context;
    let check = &discovered.check;
    let mut steps = vec![crate::workflow::wire_w1::checkout_step_for(check.history)?];
    steps.extend(acquire);
    steps.push(crate::matrix_step::download_plan_step()?);
    let image_check =
        policy == WorkflowPolicy::VelnorRepositoryV1 && check.id == "native-image-candidates";
    let execute = execute_check_step(discovered, catalog, helper_version, image_check)?;
    if image_check {
        if check.runner.platform.release_target()
            != velnor_actions_contract::ReleaseTarget::LinuxX86_64
        {
            return Err(OrchestratorError::Contract {
                problem: "native_image_check_target".to_owned(),
            });
        }
        let approved = approved_release.ok_or_else(|| OrchestratorError::Contract {
            problem: "controller_approvals:missing_approval".to_owned(),
        })?;
        let runtime = approved
            .mise_runtime
            .iter()
            .find(|runtime| runtime.target == velnor_actions_contract::ReleaseTarget::LinuxX86_64)
            .ok_or_else(|| OrchestratorError::Contract {
                problem: "approved_check_mise_missing".to_owned(),
            })?;
        steps.push(approved_check_mise_step(runtime)?);
    }
    steps.push(execute);
    if image_check {
        let pins = crate::product_release_pins::resolve(config, catalog)?;
        steps.push(crate::workflow::verify_restored_rust_step(catalog));
        steps.push(crate::matrix_step::prepare_crate_tools_step(catalog, lane)?);
        steps.extend(crate::matrix_step::verify_validators_step(catalog, lane));
        steps.push(crate::workflow::prepare_rust_components_step(catalog)?);
        steps.push(crate::workflow::verify_rust_manifest_step(catalog));
        steps
            .extend(velnor_actions_workflow_renderer::schema2::sdk_ci_image_steps(&pins, catalog)?);
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

fn approved_check_mise_command(command: Vec<String>) -> Vec<String> {
    [
        "env".to_owned(),
        "PATH=${RUNNER_TEMP}/velnor/check-mise/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin"
            .to_owned(),
    ]
    .into_iter()
    .chain(command)
    .collect()
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
    crate::attach::published_lock_acquire_for_runner(lock, runner, helper_version)
}

fn execute_check_step(
    discovered: &DiscoveredCheck,
    catalog: &ToolCatalog,
    helper_version: &str,
    approved_runtime: bool,
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
    let command = vec![crate::matrix_step::helper_path_for_version(helper_version)];
    let command = if approved_runtime {
        approved_check_mise_command(command)
    } else {
        command
    };
    velnor_actions_workflow_renderer::shell_step("Execute named check", command, env)
        .map_err(OrchestratorError::from)
}

fn approved_check_mise_step(
    runtime: &velnor_actions_mise::catalog::lock::ApprovedMiseRuntime,
) -> Result<Step, OrchestratorError> {
    let version = &runtime.version;
    let hash = &runtime.sha256;
    if version.split('.').count() != 3
        || version
            .split('.')
            .any(|part| part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()))
        || hash.len() != 64
        || !hash
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        || !(1..=268_435_456).contains(&runtime.bytes)
    {
        return Err(OrchestratorError::Contract {
            problem: "invalid_approved_check_mise".to_owned(),
        });
    }
    let size = runtime.bytes;
    let script = format!(
        "set -eu; umask 077; mkdir -p \"$RUNNER_TEMP/velnor\"; [ ! -L \"$RUNNER_TEMP/velnor\" ]; [ -O \"$RUNNER_TEMP/velnor\" ]; chmod 700 \"$RUNNER_TEMP/velnor\"; mkdir -m 700 \"$RUNNER_TEMP/velnor/check-mise\"; mkdir -m 700 \"$RUNNER_TEMP/velnor/check-mise/bin\"; /usr/bin/curl -q --fail --silent --show-error --location --proto '=https' --proto-redir '=https' --tlsv1.2 --connect-timeout 15 --max-time 600 --max-filesize {size} 'https://github.com/jdx/mise/releases/download/v{version}/mise-v{version}-linux-x64' -o \"$RUNNER_TEMP/velnor/check-mise/bin/mise\"; /usr/bin/openssl dgst -sha256 -r \"$RUNNER_TEMP/velnor/check-mise/bin/mise\" | /usr/bin/grep -qxF \"{hash} *$RUNNER_TEMP/velnor/check-mise/bin/mise\"; /usr/bin/wc -c < \"$RUNNER_TEMP/velnor/check-mise/bin/mise\" | /usr/bin/tr -d '[:space:]' | /usr/bin/grep -qxF '{size}'; chmod 0500 \"$RUNNER_TEMP/velnor/check-mise/bin/mise\""
    );
    velnor_actions_workflow_renderer::shell_step(
        "Acquire approved check Mise",
        vec!["bash".to_owned(), "-c".to_owned(), script],
        BTreeMap::new(),
    )
    .map_err(OrchestratorError::from)
}

pub(super) fn helper_catalog(
    input: &super::workflow_build::WorkflowBuildInput<'_>,
    catalog: &ToolCatalog,
) -> Result<ToolCatalog, OrchestratorError> {
    if input.config.workflow.policy != WorkflowPolicy::VelnorRepositoryV1 {
        return Ok(catalog.clone());
    }
    let lock = input
        .published_generator_lock
        .ok_or_else(|| OrchestratorError::Contract {
            problem: "required_verdict_requires_published_generator_lock".to_owned(),
        })?;
    let map_err =
        |error: velnor_actions_mise::catalog::lock::LockError| OrchestratorError::Contract {
            problem: error.to_string(),
        };
    match input.approved_release {
        Some(approved) => velnor_actions_mise::catalog::lock::catalog_for_approved_generator(
            lock, catalog, &approved,
        )
        .map_err(map_err),
        None => Err(OrchestratorError::Contract {
            problem: "controller_approvals:missing_approval".to_owned(),
        }),
    }
}
