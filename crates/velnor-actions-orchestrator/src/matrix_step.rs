//! Crate-job step constructors: prelude plus validated obligation env.

use std::collections::BTreeMap;

use velnor_actions_contract::{Step, StepKind};
use velnor_actions_mise::{
    ISOLATION_ENV, NO_AUTO_INSTALL_ENV, PREPARE_PINNED_TOOLS_STEP, PinnedTool, PreparePinnedTools,
    ToolCatalog, ToolHomes,
};

use crate::OrchestratorError;
use crate::utf8::{strings_of, strings_of_env};

/// Crate-job driver tools: Rust plus MBX only on MBX evidence.
#[must_use]
pub(crate) fn task_driver_tools(use_mbx: bool) -> Vec<PinnedTool> {
    let mut tools = vec![PinnedTool::Rust];
    tools.extend(use_mbx.then_some(PinnedTool::MrBoxington));
    tools
}

/// Typed `Prepare pinned tools` step for the crate-job tool set.
///
/// Driver toolchain plus the `generate` validators plus Nextest when used.
/// The validators install here because crate test suites execute `generate`
/// (CLI parity) and staged-validation binaries (zizmor staging) directly:
/// run 36751323928 failed every such leg with `couldn't exec process` when
/// only the plan job carried them. Dedicated validator jobs remain the lint
/// gates for the committed workflow; this set covers what the job executes,
/// tests included. Order follows `PinnedTool::ALL`.
///
/// # Errors
///
/// Returns a contract error when the Mise adapter rejects the request.
pub(crate) fn prepare_crate_tools_step(
    catalog: &ToolCatalog,
    use_mbx: bool,
    use_nextest: bool,
) -> Result<Step, OrchestratorError> {
    let mut tools = task_driver_tools(use_mbx);
    tools.extend([
        PinnedTool::Actionlint,
        PinnedTool::Shellcheck,
        PinnedTool::Zizmor,
    ]);
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

/// Validated env every crate-job Cargo step runs with.
///
/// Single constructor shared by obligation steps and `Fetch Cargo
/// sources`: the isolation quartet plus install disable from the Mise
/// adapter's single source, the owned-homes triple, and caller extras.
/// Reserved keys in the extras fail closed, so generated steps use the
/// same validated contract as local helper requests and fetch plus
/// consumers can never drift apart (run 36560676954 failed every leg
/// when only `Run task` carried the triple).
///
/// # Errors
///
/// Returns a contract error for reserved extras and a render error for
/// blank triple inputs or denied credential keys.
pub(crate) fn task_step_env(
    catalog: &ToolCatalog,
    extra: &BTreeMap<String, String>,
) -> Result<BTreeMap<String, String>, OrchestratorError> {
    use velnor_actions_workflow_renderer::toolchain_env;
    for key in extra.keys() {
        if velnor_actions_mise::command::is_reserved_env_key(key) {
            return Err(OrchestratorError::Contract {
                problem: format!("reserved_step_env:{key}"),
            });
        }
    }
    let mut base = BTreeMap::new();
    for (key, value) in ISOLATION_ENV.iter().chain(NO_AUTO_INSTALL_ENV.iter()) {
        base.insert((*key).to_owned(), (*value).to_owned());
    }
    base.extend(
        extra
            .iter()
            .map(|(key, value)| (key.clone(), value.clone())),
    );
    let homes = ToolHomes::runner_temp();
    let toolchain = catalog.rustup_toolchain();
    toolchain_env::checked_task_env(&base, homes.rustup_home(), homes.cargo_home(), &toolchain)
        .map_err(OrchestratorError::from)
}

/// Fixed obligation identity env carried by every obligation step.
///
/// Binds the step to its task, digest, and matrix coordinates with
/// fixed values (never `${{ }}` expressions), so the renderer wraps
/// each obligation with report capture and merge locates its evidence.
pub(crate) fn obligation_identity_env(
    task_id: &str,
    task_digest: &str,
    matrix_id: &str,
    matrix_key: &str,
) -> BTreeMap<String, String> {
    use velnor_actions_workflow_renderer::task_steps as legs;
    BTreeMap::from([
        (legs::LEG_TASK_ID_ENV.to_owned(), task_id.to_owned()),
        (legs::LEG_TASK_DIGEST_ENV.to_owned(), task_digest.to_owned()),
        (legs::LEG_MATRIX_ID_ENV.to_owned(), matrix_id.to_owned()),
        (legs::LEG_MATRIX_KEY_ENV.to_owned(), matrix_key.to_owned()),
    ])
}
