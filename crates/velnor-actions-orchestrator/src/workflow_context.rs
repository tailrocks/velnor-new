//! Renderer context construction from discovered repository evidence.

use velnor_actions_contract::{GeneratorValidation, ValidatorKind, VelnorConfig, WorkflowPolicy};
use velnor_actions_mise::{IsolatedCommand, PinnedTool, ToolCatalog};
use velnor_actions_workflow_renderer::render::{RenderContext, ValidatorCommand};
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
    version: &str,
    catalog: &ToolCatalog,
    discovery: &Discovery,
    plan_needs_rust: bool,
    workflow_tasks: Vec<WorkflowTaskPolicy>,
    verify: &[ValidatorKind],
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
            });
        }
        let machete_install = machete_install_argv()?;
        let zizmor_install = IsolatedCommand::mise_install(&[format!(
            "zizmor@{}",
            catalog.version(PinnedTool::Zizmor)
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
                argv: zizmor_argv(catalog)?,
                prepare_argv: zizmor_install,
            },
        ]);
    }
    crate::verify::push_verify_commands(&mut validator_commands, verify, catalog)?;
    let candidate =
        if velnor && config.workflow.generator_validation == GeneratorValidation::Candidate {
            Some(candidate_spec(catalog)?)
        } else {
            None
        };
    Ok(RenderContext {
        generator_version: version.to_owned(),
        runs_on: label.to_owned(),
        staged_binary: format!("{STAGED_BINARY_PREFIX}{version}"),
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
