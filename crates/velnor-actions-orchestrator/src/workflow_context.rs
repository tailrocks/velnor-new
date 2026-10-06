//! Render context and actionlint input for the main workflow.

use velnor_actions_actionlint::{ActionlintConfigInput, IgnorePolicy};
use velnor_actions_contract::{GeneratorValidation, ValidatorKind, VelnorConfig, WorkflowPolicy};
use velnor_actions_mise::{RuntimePaths, ToolCatalog};
use velnor_actions_workflow_renderer::render::{RenderContext, ValidatorCommand, WORKFLOW_PATH};
use velnor_actions_workflow_renderer::steps::{
    DENY_STEP_NAME, MACHETE_STEP_NAME, REQUEST_DIR_PREFIX, STAGED_BINARY_PREFIX,
};

use crate::OrchestratorError;
use crate::vectors::{ZIZMOR_STEP_NAME, candidate_spec, deny_argv, machete_argv, zizmor_argv};

use super::{CHECKOUT_USES, REQUEST_DIR};

#[path = "workflow_source_helpers.rs"]
mod source_helpers;

pub(crate) use source_helpers::{collect_rust_job_helpers, collect_rust_source_helpers};

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
    let mut plan_consumer_env = crate::matrix_step::task_step_env(
        catalog,
        &std::collections::BTreeMap::new(),
        plan_needs_rust,
    )?;
    if !velnor {
        let (key, value) = RuntimePaths::planning().mise_data_env();
        plan_consumer_env.insert(key.to_owned(), value.to_owned());
    }
    Ok(RenderContext {
        source_helpers: Vec::new(),
        native_pages_approvals: Vec::new(),
        native_publish_approvals: Vec::new(),
        generator_version: version.to_owned(),
        runs_on: label.to_owned(),
        staged_binary: format!("{STAGED_BINARY_PREFIX}{version}"),
        request_dir: REQUEST_DIR.to_owned(),
        checkout_uses: CHECKOUT_USES.to_owned(),
        validator_commands,
        candidate,
        preseed: false,
        plan_consumer_env,
    })
}

/// Actionlint input: generated workflow path plus policy-graded ignores.
///
/// The bridge label is the effective configured runner label (F2):
/// the emitted `self-hosted-runner` entry must match `runs-on`, never
/// a hardcoded distro.
pub(super) fn actionlint_input(
    policy: WorkflowPolicy,
    version: &str,
    label: &str,
) -> ActionlintConfigInput {
    let mut input = ActionlintConfigInput::new(version)
        .with_workflow_path(WORKFLOW_PATH)
        .with_config_variables(super::wire_w1::declared_config_variables())
        .with_runner_label(label);
    input.policy = match policy {
        WorkflowPolicy::ConsumerV1 => IgnorePolicy::Consumer,
        WorkflowPolicy::VelnorRepositoryV1 => IgnorePolicy::VelnorProtected,
    };
    input
}
