//! Renderer-specific context assembly for workflow plans.

use velnor_actions_actionlint::{ActionlintConfigInput, IgnorePolicy};
use velnor_actions_contract::{GeneratorValidation, ValidatorKind, VelnorConfig, WorkflowPolicy};
use velnor_actions_mise::ToolCatalog;
use velnor_actions_workflow_renderer::render::{RenderContext, ValidatorCommand, WORKFLOW_PATH};
use velnor_actions_workflow_renderer::steps::{
    DENY_STEP_NAME, MACHETE_STEP_NAME, REQUEST_DIR_PREFIX, STAGED_BINARY_PREFIX,
};

use crate::OrchestratorError;
use crate::vectors::{ZIZMOR_STEP_NAME, candidate_spec, deny_argv, machete_argv, zizmor_argv};

use super::{CHECKOUT_USES, REQUEST_DIR, wire_w1};

/// Renderer scalars: version, label, staged path, request dir, pins.
///
/// The plan-consumer env follows the plan role: plans without Rust run
/// the plan-op and freshness steps without the Rust owned-homes triple.
pub(super) fn render_context(
    config: &VelnorConfig,
    label: &str,
    version: &str,
    catalog: &ToolCatalog,
    plan_needs_rust: bool,
) -> Result<RenderContext, OrchestratorError> {
    debug_assert!(REQUEST_DIR.starts_with(REQUEST_DIR_PREFIX));
    let velnor = config.workflow.policy == WorkflowPolicy::VelnorRepositoryV1;
    let validator_commands = if velnor {
        vec![
            ValidatorCommand {
                validator: ValidatorKind::CargoDeny,
                name: DENY_STEP_NAME.to_owned(),
                argv: deny_argv()?,
            },
            ValidatorCommand {
                validator: ValidatorKind::CargoMachete,
                name: MACHETE_STEP_NAME.to_owned(),
                argv: machete_argv()?,
            },
            ValidatorCommand {
                validator: ValidatorKind::Zizmor,
                name: ZIZMOR_STEP_NAME.to_owned(),
                argv: zizmor_argv(catalog)?,
            },
        ]
    } else {
        Vec::new()
    };
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
        plan_consumer_env: crate::matrix_step::task_step_env(
            catalog,
            &std::collections::BTreeMap::new(),
            plan_needs_rust,
        )?,
    })
}

/// Actionlint input: workflow path, declared variables, and policy ignores.
///
/// The bridge label is the effective configured runner label: its
/// `self-hosted-runner` entry must match `runs-on`, never a hardcoded distro.
pub(super) fn actionlint_input(
    config: &VelnorConfig,
    version: &str,
    label: &str,
) -> ActionlintConfigInput {
    let policy = config.workflow.policy;
    let mut input = ActionlintConfigInput::new(version)
        .with_workflow_path(WORKFLOW_PATH)
        .with_config_variables(wire_w1::declared_config_variables())
        .with_runner_label(label);
    if let Some(execution) = &config.execution {
        input.extra_runner_labels = execution.actionlint_labels();
    }
    input.policy = match policy {
        WorkflowPolicy::ConsumerV1 => IgnorePolicy::Consumer,
        WorkflowPolicy::VelnorRepositoryV1 => IgnorePolicy::VelnorProtected,
    };
    input
}
