//! Task-job step constructors: prelude, sources, and matrix entry.

use std::collections::BTreeMap;

use velnor_actions_contract::{Step, StepKind};
use velnor_actions_mise::{
    PREPARE_PINNED_TOOLS_STEP, PinnedTool, PreparePinnedTools, ToolCatalog, ToolHomes,
};
use velnor_actions_workflow_renderer::render::{
    MATRIX_MAX_PARALLEL_ENV, MATRIX_NEEDS_JOB_ENV, MATRIX_OUTPUT_ENV, PLAN_JOB_ID,
};

use crate::OrchestratorError;
use crate::utf8::{strings_of, strings_of_env};

/// Producer output carrying the matrix JSON for `fromJSON`.
const MATRIX_OUTPUT_NAME: &str = "matrix";

/// Task-job driver tools: Rust plus MBX only on MBX evidence.
#[must_use]
pub(crate) fn task_driver_tools(use_mbx: bool) -> Vec<PinnedTool> {
    let mut tools = vec![PinnedTool::Rust];
    tools.extend(use_mbx.then_some(PinnedTool::MrBoxington));
    tools
}

/// Typed `Prepare pinned tools` step for the task-job tool set.
///
/// # Errors
///
/// Returns a contract error when the Mise adapter rejects the request.
pub(crate) fn prepare_task_tools_step(
    catalog: &ToolCatalog,
    use_mbx: bool,
    use_nextest: bool,
) -> Result<Step, OrchestratorError> {
    let mut tools = task_driver_tools(use_mbx);
    tools.push(PinnedTool::Actionlint);
    tools.push(PinnedTool::Shellcheck);
    tools.push(PinnedTool::Zizmor);
    tools.extend(use_nextest.then_some(PinnedTool::Nextest));
    let prepare = PreparePinnedTools::new(tools, ToolHomes::runner_temp()).map_err(|err| {
        OrchestratorError::Contract {
            problem: err.to_string(),
        }
    })?;
    let run = strings_of(prepare.argv(catalog))
        .map_err(|problem| OrchestratorError::Contract { problem })?;
    let env = strings_of_env(&prepare.env(catalog))
        .map_err(|problem| OrchestratorError::Contract { problem })?;
    Ok(Step {
        name: PREPARE_PINNED_TOOLS_STEP.to_owned(),
        kind: StepKind::Shell { run, env },
    })
}

/// Owned-homes triple every task-job Cargo step runs with.
///
/// Single constructor shared by `Run task` and `Fetch Cargo sources`:
/// fetch populates the `MISE_CARGO_HOME` the consumer reads, so the two
/// steps must never drift apart (run 36560676954 failed every leg when
/// only `Run task` carried the triple).
pub(crate) fn task_toolchain_env(catalog: &ToolCatalog) -> BTreeMap<String, String> {
    use velnor_actions_workflow_renderer::toolchain_env;
    let homes = ToolHomes::runner_temp();
    let toolchain = catalog.rustup_toolchain();
    toolchain_env::with_toolchain_homes(
        &BTreeMap::new(),
        homes.rustup_home(),
        homes.cargo_home(),
        &toolchain,
    )
}

/// Fixed matrix-entry template: run the command, always write reports.
///
/// Missing `matrix.run`/`matrix.task_digest` fail the leg via `${VAR:?...}`; env carries
/// matrix context plus the owned-homes triple, keeping `run:` free of `${{ }}` for scans.
pub(crate) fn matrix_task_step(max_parallel_jobs: u32, catalog: &ToolCatalog) -> Step {
    use velnor_actions_workflow_renderer::task_steps as legs;
    let mut env = task_toolchain_env(catalog);
    env.extend(BTreeMap::from([
        (
            legs::LEG_TASK_ID_ENV.to_owned(),
            "${{ matrix.task_id }}".to_owned(),
        ),
        (
            legs::LEG_TASK_RUN_ENV.to_owned(),
            "${{ matrix.run }}".to_owned(),
        ),
        (
            legs::LEG_TASK_DIGEST_ENV.to_owned(),
            "${{ matrix.task_digest }}".to_owned(),
        ),
        (
            legs::LEG_MATRIX_KEY_ENV.to_owned(),
            "${{ matrix.matrix_key }}".to_owned(),
        ),
        (
            legs::LEG_MATRIX_ID_ENV.to_owned(),
            "${{ matrix.id }}".to_owned(),
        ),
        (
            legs::LEG_EVENT_ENV.to_owned(),
            "${{ github.event_name }}".to_owned(),
        ),
        (MATRIX_NEEDS_JOB_ENV.to_owned(), PLAN_JOB_ID.to_owned()),
        (MATRIX_OUTPUT_ENV.to_owned(), MATRIX_OUTPUT_NAME.to_owned()),
        (
            MATRIX_MAX_PARALLEL_ENV.to_owned(),
            max_parallel_jobs.to_string(),
        ),
    ]));
    Step {
        name: "Run task".to_owned(),
        kind: StepKind::Shell {
            run: vec![
                "sh".to_owned(),
                "-c".to_owned(),
                legs::leg_execution_script(),
            ],
            env,
        },
    }
}
