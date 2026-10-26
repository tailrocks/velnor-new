// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::ffi::OsString;

use crate::plan_sources::{self, PlanSourcesError};
use crate::render::{FINAL_CONDITION, FINAL_DISPLAY_NAME, PLAN_JOB_ID};
use crate::steps::{
    MERGE_OPERATION, PLAN_OPERATION, RESOLVE_QUALIFICATION_OPERATION, merge_step, plan_step,
    write_request_step,
};
use crate::verifier_builders::{self, ToolchainPhase};
use velnor_actions_actionlint::{PinnedActionRef, checkout_inputs_schema, validate_action_inputs};
use velnor_actions_contract::workflow::permissions::PermissionLevel;
use velnor_actions_contract::{Job, JobTimeout, Permissions, Step, StepRole, WorkflowPolicy};
use velnor_actions_mise::{
    ISOLATION_ENV, NO_AUTO_INSTALL_ENV, PREPARE_PINNED_TOOLS_STEP, PinnedTool, PreparePinnedTools,
    PrepareRustComponents, ToolCatalog, ToolHomes,
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PlanRustNeed {
    #[default]
    None,
    Compiler,
    CompilerAndComponents,
}

impl PlanRustNeed {
    #[must_use]
    pub fn has_compiler(self) -> bool {
        self != Self::None
    }

    fn has_components(self) -> bool {
        self == Self::CompilerAndComponents
    }
}

#[expect(
    clippy::struct_excessive_bools,
    reason = "independent opt-in planner tool signals"
)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PlanJobToolNeeds {
    pub rust: PlanRustNeed,
    pub all_features: bool,
    pub nextest: bool,
    pub opentofu: bool,
    pub gh: bool,
    pub resolve_qualification_predecessor: bool,
}

#[derive(Debug)]
pub enum NativeJobBuilderError {
    Contract(String),
    Render(crate::RenderError),
    PlanSources(PlanSourcesError),
}

pub const LINT_JOB_ID: &str = "jactionlint";

pub fn plan_job(
    label: &str,
    acquire: Option<Step>,
    catalog: &ToolCatalog,
    needs: PlanJobToolNeeds,
    fetch_roots: &[String],
    tofu_roots: &[String],
    policy: WorkflowPolicy,
) -> Result<Job, NativeJobBuilderError> {
    let source_free = policy == WorkflowPolicy::VelnorRepositoryV1;
    let mut steps = vec![checkout_history_action()?];
    let mut acquire = acquire;
    if source_free {
        steps.extend(acquire.take());
    }
    if needs.rust.has_compiler() {
        steps.push(if source_free {
            verifier_builders::rust_verifier_shell_step(catalog, ToolchainPhase::Restored)
                .map_err(NativeJobBuilderError::Render)?
        } else {
            verifier_builders::verify_restored_rust_step()
        });
    }
    steps.push(prepare_pinned_tools_step(
        catalog,
        plan_tools(needs),
        needs.rust.has_compiler(),
    )?);
    steps.push(if source_free {
        verifier_builders::jactionlint_verifier_shell_step(catalog)
            .map_err(NativeJobBuilderError::Render)?
    } else {
        verifier_builders::verify_jactionlint_step()
    });
    if needs.rust.has_components() {
        steps.push(prepare_rust_components_step(catalog)?);
    }
    if needs.rust.has_compiler() {
        steps.push(if source_free {
            verifier_builders::rust_verifier_shell_step(catalog, ToolchainPhase::Installed)
                .map_err(NativeJobBuilderError::Render)?
        } else {
            verifier_builders::verify_rust_manifest_step()
        });
    }
    let cached = plan_sources::cache_steps_for_plan(catalog, fetch_roots, tofu_roots)
        .map_err(NativeJobBuilderError::PlanSources)?;
    steps.extend(cached.restore);
    let env = plan_fetch_env(catalog)?;
    let mut source_steps = plan_sources::cargo_fetch::fetch_steps_with(
        catalog,
        fetch_roots,
        &env,
        false,
        needs.all_features,
    )
    .map_err(NativeJobBuilderError::PlanSources)?;
    plan_sources::mark_writer_gate(&mut source_steps);
    steps.extend(source_steps);
    steps.extend(acquire);
    steps.push(request_step(PLAN_OPERATION)?);
    if needs.resolve_qualification_predecessor {
        let mut resolve = crate::steps::internal_step(
            "Resolve qualification predecessor",
            RESOLVE_QUALIFICATION_OPERATION,
        )
        .map_err(|error| NativeJobBuilderError::Contract(error.to_string()))?;
        resolve.condition = Some("github.event_name == 'workflow_dispatch'".to_owned());
        steps.push(resolve);
    }
    steps.push(plan_step());
    steps.extend(cached.save);
    Ok(Job {
        services: Vec::new(),
        display_name: "Plan".to_owned(),
        runs_on: label.to_owned(),
        check_runner: None,
        timeout_minutes: JobTimeout::PLAN,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        environment: None,
        steps,
        cache_mode: None,
    })
}

pub fn final_job(
    label: &str,
    crate_job_ids: &[String],
    acquire: Option<Step>,
) -> Result<Job, NativeJobBuilderError> {
    let mut needs = vec![PLAN_JOB_ID.to_owned()];
    needs.extend(crate_job_ids.iter().cloned());
    needs.push(LINT_JOB_ID.to_owned());
    let mut steps = vec![checkout_history_action()?];
    steps.extend(acquire);
    let pinned_tools = ToolCatalog::pinned();
    steps.push(prepare_pinned_tools_step(
        &pinned_tools,
        vec![PinnedTool::Gh],
        true,
    )?);
    steps.push(request_step(MERGE_OPERATION)?);
    steps.push(merge_step());
    Ok(Job {
        services: Vec::new(),
        display_name: FINAL_DISPLAY_NAME.to_owned(),
        runs_on: label.to_owned(),
        check_runner: None,
        timeout_minutes: JobTimeout::REQUIRED,
        needs,
        condition: Some(FINAL_CONDITION.to_owned()),
        permissions: Some(read_actions_permissions()),
        environment: None,
        steps,
        cache_mode: None,
    })
}

#[must_use]
pub fn read_actions_permissions() -> Permissions {
    Permissions {
        contents: PermissionLevel::Read,
        pull_requests: PermissionLevel::None,
        id_token: PermissionLevel::None,
        actions: PermissionLevel::Read,
        packages: PermissionLevel::None,
    }
}

pub fn prepare_pinned_tools_step(
    catalog: &ToolCatalog,
    tools: Vec<PinnedTool>,
    rust_homes: bool,
) -> Result<Step, NativeJobBuilderError> {
    let homes = ToolHomes::runner_temp();
    let prepare = PreparePinnedTools::new(tools, homes)
        .map_err(|error| NativeJobBuilderError::Contract(error.to_string()))?;
    let run = strings_of(prepare.argv(catalog)).map_err(NativeJobBuilderError::Contract)?;
    let env = if rust_homes {
        strings_of_env(&prepare.env(catalog))
    } else {
        strings_of_env(&prepare.env_without_homes())
    }
    .map_err(NativeJobBuilderError::Contract)?;
    let mut step = crate::ambient_shell_step(PREPARE_PINNED_TOOLS_STEP, run, env)
        .map_err(|error| NativeJobBuilderError::Contract(error.to_string()))?;
    step.role = Some(StepRole::PreparePinnedTools);
    Ok(step)
}

pub fn prepare_rust_components_step(catalog: &ToolCatalog) -> Result<Step, NativeJobBuilderError> {
    let request = PrepareRustComponents::new(ToolHomes::runner_temp());
    let run = strings_of(request.argv(catalog)).map_err(NativeJobBuilderError::Contract)?;
    let env = strings_of_env(&request.env(catalog)).map_err(NativeJobBuilderError::Contract)?;
    let mut step =
        crate::ambient_shell_step(velnor_actions_mise::PREPARE_RUST_COMPONENTS_STEP, run, env)
            .map_err(NativeJobBuilderError::Render)?;
    step.role = Some(StepRole::PrepareRustComponents);
    Ok(step)
}

fn plan_tools(needs: PlanJobToolNeeds) -> Vec<PinnedTool> {
    let mut tools = Vec::new();
    tools.extend(needs.rust.has_compiler().then_some(PinnedTool::Rust));
    tools.extend([PinnedTool::Jactionlint, PinnedTool::Shellcheck]);
    tools.extend(needs.nextest.then_some(PinnedTool::Nextest));
    tools.extend(needs.opentofu.then_some(PinnedTool::Opentofu));
    tools.extend(needs.gh.then_some(PinnedTool::Gh));
    tools
}

fn plan_fetch_env(
    catalog: &ToolCatalog,
) -> Result<BTreeMap<String, String>, NativeJobBuilderError> {
    let mut base = BTreeMap::new();
    for (key, value) in ISOLATION_ENV.iter().chain(NO_AUTO_INSTALL_ENV.iter()) {
        base.insert((*key).to_owned(), (*value).to_owned());
    }
    let homes = ToolHomes::runner_temp();
    crate::toolchain_env::checked_task_env(
        &base,
        homes.rustup_home(),
        homes.cargo_home(),
        catalog.rustup_toolchain_env().unwrap_or(""),
    )
    .map_err(NativeJobBuilderError::Render)
}

fn request_step(target: &str) -> Result<Step, NativeJobBuilderError> {
    write_request_step(target).map_err(|error| NativeJobBuilderError::Contract(error.to_string()))
}

fn checkout_history_action() -> Result<Step, NativeJobBuilderError> {
    let with = BTreeMap::from([
        ("persist-credentials".to_owned(), "false".to_owned()),
        ("fetch-depth".to_owned(), "0".to_owned()),
    ]);
    validate_action_inputs(&checkout_inputs_schema(), &with)
        .map_err(|error| NativeJobBuilderError::Contract(error.to_string()))?;
    let uses = PinnedActionRef::checkout().uses_value();
    let mut step = crate::steps::action_step("Checkout", &uses, with)
        .map_err(|error| NativeJobBuilderError::Contract(error.to_string()))?;
    step.role = Some(StepRole::Checkout);
    Ok(step)
}

fn strings_of(argv: Vec<OsString>) -> Result<Vec<String>, String> {
    let mut output = Vec::with_capacity(argv.len());
    for arg in argv {
        match arg.into_string() {
            Ok(value) => output.push(value),
            Err(_) => return Err("non_utf8_argv".to_owned()),
        }
    }
    Ok(output)
}

fn strings_of_env(env: &[(OsString, OsString)]) -> Result<BTreeMap<String, String>, String> {
    let mut output = BTreeMap::new();
    for (key, value) in env {
        let (Some(key), Some(value)) = (key.to_str(), value.to_str()) else {
            return Err("non_utf8_env".to_owned());
        };
        output.insert(key.to_owned(), value.to_owned());
    }
    Ok(output)
}
