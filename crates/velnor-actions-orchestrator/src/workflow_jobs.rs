// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::ffi::OsString;

use velnor_actions_contract::{Job, JobTimeout, Permissions, Step, StepRole, WorkflowPolicy};
use velnor_actions_mise::{PinnedTool, PinnedToolExec, ToolCatalog};
use velnor_actions_workflow_renderer::native_job_builders::NativeJobBuilderError;

use crate::OrchestratorError;
use crate::utf8::strings_of;

pub(crate) use velnor_actions_workflow_renderer::native_job_builders::{
    PlanJobToolNeeds, PlanRustNeed,
};

pub(crate) const LINT_JOB_ID: &str =
    velnor_actions_workflow_renderer::native_job_builders::LINT_JOB_ID;
pub(crate) const LINT_DISPLAY_NAME: &str = "Jactionlint";

pub(crate) fn plan_job(
    label: &str,
    acquire: Option<Step>,
    catalog: &ToolCatalog,
    needs: PlanJobToolNeeds,
    fetch_roots: &[String],
    tofu_roots: &[String],
    policy: WorkflowPolicy,
) -> Result<Job, OrchestratorError> {
    velnor_actions_workflow_renderer::native_job_builders::plan_job(
        label,
        acquire,
        catalog,
        needs,
        fetch_roots,
        tofu_roots,
        policy,
    )
    .map_err(map_native_job_builder_error)
}

pub(crate) fn lint_job(label: &str, catalog: &ToolCatalog) -> Result<Job, OrchestratorError> {
    let argv = workflow_lint_argv(catalog)?;
    let prepare = prepare_pinned_tools_step(
        catalog,
        vec![PinnedTool::Jactionlint, PinnedTool::Shellcheck],
        false,
    )?;
    let verify = crate::workflow::verify_jactionlint_step(catalog);
    let mut lint =
        velnor_actions_workflow_renderer::shell_step("Run jactionlint", argv, BTreeMap::new())
            .map_err(|err| OrchestratorError::Contract {
                problem: err.to_string(),
            })?;
    lint.role = Some(StepRole::Jactionlint);
    Ok(Job {
        services: Vec::new(),
        display_name: LINT_DISPLAY_NAME.to_owned(),
        runs_on: label.to_owned(),
        check_runner: None,
        timeout_minutes: JobTimeout::VALIDATOR,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        environment: None,
        steps: vec![checkout_action()?, prepare, verify, lint],
        cache_mode: None,
    })
}

pub(crate) fn workflow_lint_argv(catalog: &ToolCatalog) -> Result<Vec<String>, OrchestratorError> {
    let exec = PinnedToolExec::new(
        vec![PinnedTool::Jactionlint, PinnedTool::Shellcheck],
        &OsString::from("jactionlint"),
        Vec::new(),
    )
    .map_err(|err| OrchestratorError::Contract {
        problem: err.to_string(),
    })?;
    let command = strings_of(exec.argv(catalog))
        .map_err(|problem| OrchestratorError::Contract { problem })?
        .join(" ");
    Ok(vec![
        "bash".to_owned(),
        "-c".to_owned(),
        velnor_actions_actionlint::workflow_lint_script(&command),
    ])
}

pub(crate) fn final_job(
    label: &str,
    crate_job_ids: &[String],
    acquire: Option<Step>,
    catalog: &ToolCatalog,
) -> Result<Job, OrchestratorError> {
    velnor_actions_workflow_renderer::native_job_builders::final_job(
        label,
        crate_job_ids,
        acquire,
        catalog,
    )
    .map_err(map_native_job_builder_error)
}

pub(crate) fn read_actions_permissions() -> Permissions {
    velnor_actions_workflow_renderer::native_job_builders::read_actions_permissions()
}

pub(crate) fn prepare_pinned_tools_step(
    catalog: &ToolCatalog,
    tools: Vec<PinnedTool>,
    rust_homes: bool,
) -> Result<Step, OrchestratorError> {
    velnor_actions_workflow_renderer::native_job_builders::prepare_pinned_tools_step(
        catalog, tools, rust_homes,
    )
    .map_err(map_native_job_builder_error)
}

pub(crate) fn prepare_rust_components_step(
    catalog: &ToolCatalog,
) -> Result<Step, OrchestratorError> {
    velnor_actions_workflow_renderer::native_job_builders::prepare_rust_components_step(catalog)
        .map_err(map_native_job_builder_error)
}

pub(crate) fn map_native_job_builder_error(error: NativeJobBuilderError) -> OrchestratorError {
    match error {
        NativeJobBuilderError::Contract(problem) => OrchestratorError::Contract { problem },
        NativeJobBuilderError::Render(error) => OrchestratorError::from(error),
        NativeJobBuilderError::PlanSources(error) => match error {
            velnor_actions_workflow_renderer::plan_sources::PlanSourcesError::Contract(error) => {
                OrchestratorError::from(error)
            }
            velnor_actions_workflow_renderer::plan_sources::PlanSourcesError::ShellRender(error) => {
                OrchestratorError::Contract {
                    problem: error.to_string(),
                }
            }
            velnor_actions_workflow_renderer::plan_sources::PlanSourcesError::CacheSourcesPolicy(
                problem,
            ) => OrchestratorError::Contract { problem },
            velnor_actions_workflow_renderer::plan_sources::PlanSourcesError::CacheActionRef(
                problem,
            ) => OrchestratorError::WorkflowLint { problem },
            velnor_actions_workflow_renderer::plan_sources::PlanSourcesError::Render(error) => {
                OrchestratorError::from(error)
            }
        },
    }
}

fn checkout_action() -> Result<Step, OrchestratorError> {
    crate::workflow::wire_w1::checkout_step()
}
