//! Renderer context construction from discovered repository evidence.

use velnor_actions_contract::{
    ExecutionMode, GeneratorValidation, ScaleSetSelector, ValidatorKind, VelnorConfig,
    VerifyConfig, WorkflowPolicy,
};
use velnor_actions_mise::ToolCatalog;
use velnor_actions_workflow_renderer::render::{RenderContext, ValidatorCommand};
use velnor_actions_workflow_renderer::steps::{
    DENY_STEP_NAME, MACHETE_STEP_NAME, REQUEST_DIR_PREFIX, STAGED_BINARY_PREFIX,
};
use velnor_actions_workflow_renderer::verification_jobs::WorkflowTaskPolicy;

use crate::OrchestratorError;
use crate::discover::Discovery;
use crate::vectors::{
    ZIZMOR_STEP_NAME, candidate_spec, deny_argv, machete_argv, machete_install_argv, zizmor_argv,
    zizmor_install_argv,
};

use super::{CHECKOUT_USES, REQUEST_DIR};

/// Renderer scalars: version, label, staged path, request dir, pins.
///
/// The plan-consumer env follows the plan role: pure-tofu plans run
/// the plan-op and freshness steps triple-less, every other role
/// keeps the owned-homes triple.
#[expect(
    clippy::too_many_arguments,
    reason = "verify selection rides with the context inputs"
)]
pub(super) fn render_context(
    config: &VelnorConfig,
    label: &str,
    generator_version: &str,
    report_helper_version: &str,
    catalog: &ToolCatalog,
    discovery: &Discovery,
    plan_needs_rust: bool,
    workflow_tasks: Vec<WorkflowTaskPolicy>,
    verify: &[ValidatorKind],
    verify_config: &VerifyConfig,
) -> Result<RenderContext, OrchestratorError> {
    debug_assert!(REQUEST_DIR.starts_with(REQUEST_DIR_PREFIX));
    let velnor = config.workflow.policy == WorkflowPolicy::VelnorRepositoryV1;
    let mut validator_commands = Vec::new();
    if velnor {
        let workspaces = discovery
            .workspaces
            .iter()
            .map(|workspace| workspace.record.workspace_root.clone())
            .collect::<Vec<_>>();
        if !workspaces.is_empty() {
            validator_commands.push(ValidatorCommand {
                validator: ValidatorKind::CargoDeny,
                name: DENY_STEP_NAME.to_owned(),
                argv: deny_argv(&workspaces)?,
                prepare_argv: Vec::new(),
                source_units: Vec::new(),
                tool_inputs: Vec::new(),
            });
        }
        validator_commands.extend([
            ValidatorCommand {
                validator: ValidatorKind::CargoMachete,
                name: MACHETE_STEP_NAME.to_owned(),
                argv: machete_argv()?,
                prepare_argv: machete_install_argv()?,
                source_units: Vec::new(),
                tool_inputs: Vec::new(),
            },
            ValidatorCommand {
                validator: ValidatorKind::Zizmor,
                name: ZIZMOR_STEP_NAME.to_owned(),
                argv: zizmor_argv(catalog)?,
                prepare_argv: zizmor_install_argv(catalog)?,
                source_units: Vec::new(),
                tool_inputs: Vec::new(),
            },
        ]);
        validator_commands.push(crate::python_source_units::validator_command(catalog)?);
    }
    crate::verify::push_verify_commands(&mut validator_commands, verify, verify_config, catalog)?;
    let candidate =
        if velnor && config.workflow.generator_validation == GeneratorValidation::Candidate {
            Some(candidate_spec(catalog)?)
        } else {
            None
        };
    Ok(RenderContext {
        generator_version: generator_version.to_owned(),
        report_helper_version: report_helper_version.to_owned(),
        runs_on: label.to_owned(),
        scale_set_selector: scale_set_selector(config, None)?,
        staged_binary: format!("{STAGED_BINARY_PREFIX}{report_helper_version}"),
        request_dir: REQUEST_DIR.to_owned(),
        checkout_uses: CHECKOUT_USES.to_owned(),
        validator_commands,
        candidate,
        preseed: false,
        workflow_tasks,
        pull_request_cache_policy: config.workflow.pull_request_cache_policy,
        plan_consumer_env: crate::matrix_step::task_step_env(
            catalog,
            &std::collections::BTreeMap::new(),
            plan_needs_rust,
        )?,
    })
}

pub(super) fn scale_set_selector(
    config: &VelnorConfig,
    dispatch: Option<ExecutionMode>,
) -> Result<Option<ScaleSetSelector>, OrchestratorError> {
    let selector = config
        .execution
        .as_ref()
        .map(|execution| {
            execution
                .validate("config.toml")
                .map_err(|error| OrchestratorError::Contract {
                    problem: error.to_string(),
                })?;
            let mut effective = execution.clone();
            if let Some(mode) = dispatch {
                effective.mode = Some(mode);
            }
            if !effective.emits_scale_set_selector() {
                return Ok(None);
            }
            effective
                .scale_selector()
                .map(Some)
                .map_err(|error| OrchestratorError::Contract {
                    problem: error.to_string(),
                })
        })
        .transpose()?;
    Ok(selector.flatten())
}
