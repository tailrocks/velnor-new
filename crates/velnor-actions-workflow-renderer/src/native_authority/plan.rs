// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{NativeAuthorityError, NativeAuthorityInput, WorkspaceFormatFact, invalid};
use std::collections::BTreeMap;
use std::ffi::OsString;
use velnor_actions_contract::{Job, ProposedTask, Step, StepKind, StepRole};
use velnor_actions_mise::{PinnedToolExec, RouteDriver, ToolCatalog, ToolHomes};

pub(super) fn build(
    input: &NativeAuthorityInput<'_>,
    catalog: &ToolCatalog,
    acquire: Step,
    fetch: &[String],
    tofu: &[String],
    version: &str,
) -> Result<Job, NativeAuthorityError> {
    let mut job = crate::native_job_builders::plan_job(
        input.runner,
        Some(acquire),
        catalog,
        input.tools,
        fetch,
        tofu,
        input.config.workflow.policy,
    )
    .map_err(NativeAuthorityError::Job)?;
    job.timeout_minutes =
        velnor_actions_contract::JobTimeout::new(input.config.workflow.plan_timeout_minutes)?;
    job.permissions = Some(crate::native_job_builders::read_actions_permissions());
    if let Some(fact) = input.workspace_format {
        let task = format_task(fact)?;
        let driver = RouteDriver::from_compile_driver(&task.identity.compile_driver)
            .ok_or_else(|| invalid("unknown_compile_driver"))?;
        let mut tools = driver.probe_tools();
        if input.cargo_workspaces.iter().any(|workspace| {
            workspace.test_runner == velnor_actions_rust::TestRunner::CargoNextest
                && (if workspace.root.is_empty() {
                    "Cargo.toml".to_owned()
                } else {
                    format!("{}/Cargo.toml", workspace.root)
                }) == fact.manifest
        }) {
            tools.push(velnor_actions_mise::PinnedTool::Nextest);
        }
        let exec = PinnedToolExec::new(tools, &OsString::from(driver.program()), task.payload)
            .map_err(NativeAuthorityError::Tool)?;
        let argv = strings(exec.argv(catalog))?;
        let id = velnor_actions_contract::matrix_id_for_task_group(&task.stack_id, &task.task_id)?;
        let key = velnor_actions_contract::matrix_key_for_id(&id)?;
        let outcome = crate::plan_report::outcome_path_for_key(&key);
        let start = crate::plan_report::start_path_for_key(&key);
        let env = ToolHomes::runner_temp()
            .exec_env(catalog)
            .into_iter()
            .map(|(key, value)| {
                Ok((
                    key.into_string()
                        .map_err(|_| invalid("non_utf8_tool_environment"))?,
                    value
                        .into_string()
                        .map_err(|_| invalid("non_utf8_tool_environment"))?,
                ))
            })
            .collect::<Result<BTreeMap<_, _>, NativeAuthorityError>>()?;
        let format = crate::plan_format::outcome_format_step(argv, &key, &env)?;
        let helper = crate::plan_report::helper_path_for_version(version);
        let report = crate::shell_step(
            "Report Format",
            crate::plan_report::deferred_report_argv(&outcome, &helper, &start),
            BTreeMap::from([(crate::plan_report::TASK_ID_ENV.to_owned(), task.task_id)]),
        )?;
        let upload = crate::crate_job_report_upload_step(crate::render::PLAN_JOB_ID)?;
        let at = job.steps.iter().position(|step| matches!(&step.kind, StepKind::Internal { operation, .. } if operation == crate::steps::PLAN_OPERATION)).ok_or_else(|| invalid("trusted_plan_operation_count"))? + 1;
        job.steps.splice(at..at, [format, report, upload]);
    }
    Ok(job)
}

pub(super) fn format_task(
    fact: &WorkspaceFormatFact,
) -> Result<ProposedTask, NativeAuthorityError> {
    use velnor_actions_rust::{NextestProfile, ProfileSource, RustExecutionProfile, TestRunner};
    let profile = RustExecutionProfile {
        compile_driver: fact.compile_driver,
        test_runner: TestRunner::CargoTest,
        evidence: Vec::new(),
        driver_source: ProfileSource::Declared,
        runner_source: ProfileSource::Declared,
        nextest_profile: NextestProfile::Default,
        nextest_config: None,
        run_ignored: None,
    };
    Ok(velnor_actions_rust::propose_task(
        &velnor_actions_rust::derive_workspace_fmt(
            &fact.manifest,
            &profile,
            &fact.configuration,
            &fact.target,
        )?,
    )?)
}

pub(super) fn checkout_lfs(job: &mut Job) {
    for step in &mut job.steps {
        if step.role == Some(StepRole::Checkout)
            && let StepKind::Action { with, .. } = &mut step.kind
        {
            with.insert("lfs".to_owned(), "true".to_owned());
        }
    }
}

fn strings(values: Vec<OsString>) -> Result<Vec<String>, NativeAuthorityError> {
    values
        .into_iter()
        .map(|value| {
            value
                .into_string()
                .map_err(|_| invalid("non_utf8_tool_command"))
        })
        .collect()
}

pub(super) fn helper_catalog(
    input: &NativeAuthorityInput<'_>,
    catalog: &ToolCatalog,
) -> Result<ToolCatalog, NativeAuthorityError> {
    if input.config.workflow.policy != velnor_actions_contract::WorkflowPolicy::VelnorRepositoryV1 {
        return Ok(catalog.clone());
    }
    let lock = input
        .published_generator_lock
        .ok_or_else(|| invalid("required_verdict_requires_published_generator_lock"))?;
    let map_err = |error: velnor_actions_mise::catalog::lock::LockError| {
        NativeAuthorityError::Release(crate::release_helpers::ReleaseHelperError::Problem(
            error.to_string(),
        ))
    };
    match input.approved_release {
        Some(approved) => velnor_actions_mise::catalog::lock::catalog_for_approved_generator(
            lock, catalog, &approved,
        )
        .map_err(map_err),
        None => Err(invalid("controller_approvals:missing_approval")),
    }
}

pub(super) fn required_job(
    input: &NativeAuthorityInput<'_>,
    required_ids: &[String],
    acquire: Step,
) -> Result<Job, NativeAuthorityError> {
    let catalog = if input.config.workflow.policy
        == velnor_actions_contract::WorkflowPolicy::VelnorRepositoryV1
    {
        velnor_actions_mise::catalog::lock::catalog_for_published_verdict(&ToolCatalog::pinned())
    } else {
        helper_catalog(input, &ToolCatalog::pinned())?
    };
    crate::native_job_builders::final_job(input.runner, required_ids, Some(acquire), &catalog)
        .map_err(NativeAuthorityError::Job)
}
