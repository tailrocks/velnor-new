//! Renderer context construction from discovered repository evidence.

use velnor_actions_contract::{
    GeneratorValidation, ScaleSetSelector, ValidatorKind, VelnorConfig, WorkflowPolicy,
};
use velnor_actions_mise::{IsolatedCommand, PinnedTool, ToolCatalog};
use velnor_actions_workflow_renderer::render::{CandidateSpec, RenderContext, ValidatorCommand};
use velnor_actions_workflow_renderer::steps::{
    DENY_STEP_NAME, MACHETE_STEP_NAME, REQUEST_DIR_PREFIX, STAGED_BINARY_PREFIX,
};
use velnor_actions_workflow_renderer::verification_jobs::WorkflowTaskPolicy;

use crate::OrchestratorError;
use crate::discover::Discovery;
use crate::vectors::{
    ZIZMOR_STEP_NAME, candidate_spec, deny_argv, machete_argv, machete_install_argv, zizmor_argv,
};

use super::{CHECKOUT_USES, REQUEST_DIR};

/// Inputs used to derive renderer context from the workflow build.
pub(super) struct RenderContextInputs<'a> {
    pub(super) config: &'a VelnorConfig,
    pub(super) label: &'a str,
    pub(super) generator_version: &'a str,
    pub(super) report_helper_version: &'a str,
    pub(super) catalog: &'a ToolCatalog,
    pub(super) discovery: &'a Discovery,
    pub(super) plan_needs_rust: bool,
    pub(super) workflow_tasks: Vec<WorkflowTaskPolicy>,
    pub(super) verify: &'a [ValidatorKind],
}

/// Renderer scalars: generator marker version, helper version, label, request
/// directory, and pins.
///
/// The plan-consumer env follows the plan role: pure-tofu plans run
/// the plan-op and freshness steps triple-less, every other role
/// keeps the owned-homes triple.
pub(super) fn render_context(
    input: RenderContextInputs<'_>,
) -> Result<RenderContext, OrchestratorError> {
    debug_assert!(REQUEST_DIR.starts_with(REQUEST_DIR_PREFIX));
    let validator_commands = validator_commands(&input)?;
    let candidate = candidate_spec_for(&input)?;
    let scale_set_selector = scale_set_selector(input.config)?;
    Ok(RenderContext {
        generator_version: input.generator_version.to_owned(),
        report_helper_version: input.report_helper_version.to_owned(),
        runs_on: input.label.to_owned(),
        scale_set_selector,
        staged_binary: format!("{STAGED_BINARY_PREFIX}{}", input.report_helper_version),
        request_dir: REQUEST_DIR.to_owned(),
        checkout_uses: CHECKOUT_USES.to_owned(),
        validator_commands,
        candidate,
        preseed: false,
        workflow_tasks: input.workflow_tasks,
        pull_request_cache_policy: input.config.workflow.pull_request_cache_policy,
        plan_consumer_env: crate::matrix_step::task_step_env(
            input.catalog,
            &std::collections::BTreeMap::new(),
            input.plan_needs_rust,
        )?,
    })
}

fn validator_commands(
    input: &RenderContextInputs<'_>,
) -> Result<Vec<ValidatorCommand>, OrchestratorError> {
    let velnor = input.config.workflow.policy == WorkflowPolicy::VelnorRepositoryV1;
    let mut validator_commands = Vec::new();
    if velnor {
        let workspaces = input
            .discovery
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
            });
        }
        let machete_install = machete_install_argv()?;
        let zizmor_install = IsolatedCommand::mise_install(&[format!(
            "zizmor@{}",
            input.catalog.version(PinnedTool::Zizmor)
        )])
        .map_err(|err| OrchestratorError::Contract {
            problem: err.to_string(),
        })?;
        let zizmor_install = crate::utf8::strings_of(zizmor_install.argv())
            .map_err(|problem| OrchestratorError::Contract { problem })?;
        validator_commands.extend([
            ValidatorCommand {
                validator: ValidatorKind::CargoMachete,
                name: MACHETE_STEP_NAME.to_owned(),
                argv: machete_argv()?,
                prepare_argv: machete_install,
            },
            ValidatorCommand {
                validator: ValidatorKind::Zizmor,
                name: ZIZMOR_STEP_NAME.to_owned(),
                argv: zizmor_argv(input.catalog)?,
                prepare_argv: zizmor_install,
            },
        ]);
    }
    crate::verify::push_verify_commands(&mut validator_commands, input.verify, input.catalog)?;
    Ok(validator_commands)
}

fn candidate_spec_for(
    input: &RenderContextInputs<'_>,
) -> Result<Option<CandidateSpec>, OrchestratorError> {
    let is_candidate = input.config.workflow.policy == WorkflowPolicy::VelnorRepositoryV1
        && input.config.workflow.generator_validation == GeneratorValidation::Candidate;
    if is_candidate {
        candidate_spec(input.catalog).map(Some)
    } else {
        Ok(None)
    }
}

fn scale_set_selector(
    config: &VelnorConfig,
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
            if !execution.emits_scale_set_selector() {
                return Ok(None);
            }
            execution
                .scale_selector()
                .map(Some)
                .map_err(|error| OrchestratorError::Contract {
                    problem: error.to_string(),
                })
        })
        .transpose()?;
    Ok(selector.flatten())
}
