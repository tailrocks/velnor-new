//! Crate-job step constructors: prelude plus validated obligation env.

use std::collections::BTreeMap;

use velnor_actions_contract::{CrateObligation, Step, StepKind, sanitize_error_detail};
use velnor_actions_mise::{
    ISOLATION_ENV, NO_AUTO_INSTALL_ENV, PREPARE_PINNED_TOOLS_STEP, PinnedTool, PreparePinnedTools,
    ToolCatalog, ToolHomes,
};
use velnor_actions_rust::TaskKind;
use velnor_actions_workflow_renderer::plan_format::FORMAT_STEP_NAME;
use velnor_actions_workflow_renderer::steps::{INTERNAL_OP_ENV, STAGED_BINARY_PREFIX};

use crate::OrchestratorError;
use crate::task_report::{DOWNSTREAM_IDS_ENV, EXIT_CODE_ENV, REPORT_OP, TASK_ID_ENV};
use crate::utf8::{strings_of, strings_of_env};

/// `Clippy` obligation step name.
pub(crate) const CLIPPY_NAME: &str = "Clippy";
/// `Build test executables` obligation step name.
pub(crate) const BUILD_TEST_NAME: &str = "Build test executables";
/// `Unit and integration tests` obligation step name.
pub(crate) const TEST_NAME: &str = "Unit and integration tests";
/// `Doctests` obligation step name.
pub(crate) const DOCTESTS_NAME: &str = "Doctests";
/// `Documentation` obligation step name.
pub(crate) const DOCUMENTATION_NAME: &str = "Documentation";

/// Env key carrying the obligation's task ID (report lookup key).
pub(crate) const OBLIGATION_TASK_ID_ENV: &str = "VELNOR_TASK_ID";
/// Env key carrying the obligation's task digest.
pub(crate) const OBLIGATION_TASK_DIGEST_ENV: &str = "VELNOR_TASK_DIGEST";
/// Env key carrying the obligation's matrix ID.
pub(crate) const OBLIGATION_MATRIX_ID_ENV: &str = "VELNOR_MATRIX_ID";
/// Env key carrying the obligation's matrix key.
pub(crate) const OBLIGATION_MATRIX_KEY_ENV: &str = "VELNOR_MATRIX_KEY";

/// Human step name for one obligation; shards name their index.
pub(crate) fn step_name_for(kind: TaskKind, task_id: &str) -> String {
    let base = match kind {
        TaskKind::Fmt => FORMAT_STEP_NAME,
        TaskKind::Clippy => CLIPPY_NAME,
        TaskKind::Build => BUILD_TEST_NAME,
        TaskKind::Test | TaskKind::Nextest => TEST_NAME,
        TaskKind::Doctest => DOCTESTS_NAME,
        TaskKind::Doc => DOCUMENTATION_NAME,
    };
    match shard_suffix(task_id) {
        Some((index, count)) => format!("{base} (shard {index} of {count})"),
        None => base.to_owned(),
    }
}

/// Shard index/count from a trailing `/shard-<index>-of-<count>` segment.
pub(crate) fn shard_suffix(task_id: &str) -> Option<(u32, u32)> {
    let (_, index, count) = velnor_actions_contract::split_shard_suffix(task_id)?;
    Some((index, count))
}

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
/// (CLI parity) and staged-validation binaries (zizmor staging) directly,
/// and `generate` shells out to all three validators (actionlint,
/// shellcheck, zizmor): run 36751323928 failed every such leg with
/// `couldn't exec process` when only the plan job carried them. Dedicated
/// validator jobs remain the lint gates for the committed workflow; this
/// set covers what the job executes, tests included. The set is exact and
/// pinned by test: driver, validators, optional Nextest, nothing else.
/// Order follows `PinnedTool::ALL`.
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
        condition: None,
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

/// Validated env for steps executing repository code.
///
/// [`task_step_env`] plus the explicit empty credential scrub: absence
/// would inherit ambient runner and workflow-scope tokens into build
/// scripts and custom tasks. Fetch, prepare, and plan steps keep the
/// unscrubbed base: they execute no repository code and need ambient
/// auth for downloads and baseline lookups.
///
/// # Errors
///
/// Returns the same errors as [`task_step_env`].
pub(crate) fn task_execution_env(
    catalog: &ToolCatalog,
    extra: &BTreeMap<String, String>,
) -> Result<BTreeMap<String, String>, OrchestratorError> {
    use velnor_actions_workflow_renderer::toolchain_env;
    Ok(toolchain_env::with_credential_scrub(&task_step_env(
        catalog, extra,
    )?))
}

/// Fixed obligation identity env carried by every obligation step.
///
/// Binds the step to its task, digest, and matrix coordinates with
/// fixed values (never `${{ }}` expressions), so the report wrapper
/// invokes the helper for the right obligation and merge locates its
/// evidence. Downstream same-job task IDs ride along so a failure
/// reports its skipped successors; empty downstream stays absent.
pub(crate) fn obligation_identity_env(
    task_id: &str,
    task_digest: &str,
    matrix_id: &str,
    matrix_key: &str,
) -> BTreeMap<String, String> {
    BTreeMap::from([
        (OBLIGATION_TASK_ID_ENV.to_owned(), task_id.to_owned()),
        (
            OBLIGATION_TASK_DIGEST_ENV.to_owned(),
            task_digest.to_owned(),
        ),
        (OBLIGATION_MATRIX_ID_ENV.to_owned(), matrix_id.to_owned()),
        (OBLIGATION_MATRIX_KEY_ENV.to_owned(), matrix_key.to_owned()),
    ])
}

/// One obligation shell step: fixed argv wrapped with report capture.
///
/// The wrapper runs the obligation, captures `$?`, invokes the staged
/// helper's [`REPORT_OP`] with the exit code, then exits with the
/// obligation's own code (a report failure surfaces only when the
/// obligation itself passed, so failures never mask each other).
/// Identity env doubles as the report lookup key; the plan binds the
/// digests, never these baked values.
///
/// # Errors
///
/// Returns contract errors for invalid argv, joins, or wrapper shapes.
pub(crate) fn obligation_step(
    obligation: &CrateObligation,
    catalog: &ToolCatalog,
    downstream: &[String],
) -> Result<Step, OrchestratorError> {
    let matrix_id = velnor_actions_contract::matrix_id_for_task_group(
        velnor_actions_rust::STACK_ID,
        &obligation.task_id,
    )
    .map_err(crate::internal::internal_contract)?;
    let mut identity = obligation_identity_env(
        &obligation.task_id,
        &obligation.task_digest,
        &matrix_id,
        &obligation.matrix_key,
    );
    if !downstream.is_empty() {
        identity.insert(DOWNSTREAM_IDS_ENV.to_owned(), downstream.join(","));
    }
    check_identity_env_contract(&identity, &obligation.task_id)?;
    let env = task_execution_env(catalog, &identity)?;
    let joined =
        velnor_actions_workflow_renderer::join_argv_for_run(&obligation.run).map_err(|err| {
            OrchestratorError::Contract {
                problem: err.to_string(),
            }
        })?;
    let run = report_wrapper_argv(&joined, &helper_path_for_version());
    velnor_actions_workflow_renderer::validate_command_argv(&run).map_err(|err| {
        OrchestratorError::Contract {
            problem: err.to_string(),
        }
    })?;
    Ok(Step {
        name: obligation.step_name.clone(),
        condition: None,
        kind: StepKind::Shell { run, env },
    })
}

/// Require the obligation identity env to match the report op contract.
///
/// The wrapper resolves its obligation through [`TASK_ID_ENV`]; the
/// emitted map must carry that key with the exact task ID in every
/// build, or the report would bind the wrong obligation (or none).
///
/// # Errors
///
/// Returns a contract error when the lookup key is missing or differs.
pub(crate) fn check_identity_env_contract(
    identity: &BTreeMap<String, String>,
    task_id: &str,
) -> Result<(), OrchestratorError> {
    if identity.get(TASK_ID_ENV).map(String::as_str) != Some(task_id) {
        return Err(OrchestratorError::Contract {
            problem: format!(
                "obligation_identity_mismatch:{}",
                sanitize_error_detail(task_id)
            ),
        });
    }
    Ok(())
}

/// Staged helper path for this generator version (uniform preseed/consumer).
pub(crate) fn helper_path_for_version() -> String {
    format!("{STAGED_BINARY_PREFIX}{}", env!("CARGO_PKG_VERSION"))
}

/// `sh -c` argv wrapping one joined command with report capture.
///
/// Captures the obligation exit, reports through the staged helper,
/// then exits with the obligation code (helper failure surfaces only
/// on an otherwise passing obligation). The unset prelude runs first:
/// obligations execute repository code (build scripts), and the step
/// env cannot shadow runner-injected credentials (D3).
pub(crate) fn report_wrapper_argv(joined: &str, helper: &str) -> Vec<String> {
    use velnor_actions_workflow_renderer::toolchain_env::with_credential_unset_script;
    vec![
        "sh".to_owned(),
        "-c".to_owned(),
        with_credential_unset_script(&format!(
            "{joined}; code=$?; {EXIT_CODE_ENV}=\"$code\" {INTERNAL_OP_ENV}={REPORT_OP} \"{helper}\"; helper_code=$?; if [ \"$code\" -ne 0 ]; then exit \"$code\"; fi; exit \"$helper_code\""
        )),
    ]
}

/// `sh -c` argv saving one joined command's exit to an outcome file.
///
/// Two-phase shape for the plan-job workspace Format: the plan does
/// not exist yet at format time, so the wrapper records `$?` and a
/// post-plan step reports through [`deferred_report_argv`].
pub(crate) fn outcome_wrapper_argv(joined: &str, outcome_path: &str) -> Vec<String> {
    vec![
        "sh".to_owned(),
        "-c".to_owned(),
        format!("{joined}; code=$?; echo \"$code\" > \"{outcome_path}\"; exit \"$code\""),
    ]
}

/// `sh -c` argv reporting one saved outcome through the staged helper.
///
/// Reads the exit code the outcome wrapper saved (a missing file
/// leaves the code empty and the helper fails closed), then invokes
/// [`REPORT_OP`]; the step exits with the helper's own code.
pub(crate) fn deferred_report_argv(outcome_path: &str, helper: &str) -> Vec<String> {
    vec![
        "sh".to_owned(),
        "-c".to_owned(),
        format!(
            "read -r code rest < \"{outcome_path}\"; {EXIT_CODE_ENV}=\"$code\" {INTERNAL_OP_ENV}={REPORT_OP} \"{helper}\""
        ),
    ]
}

/// Shell-spelled outcome file for one matrix key under runner temp.
pub(crate) fn outcome_path_for_key(matrix_key: &str) -> String {
    format!("$RUNNER_TEMP/velnor/outcome-{matrix_key}")
}

/// Plan-artifact download: report wrappers resolve identities from it.
pub(crate) fn download_plan_step() -> Result<Step, OrchestratorError> {
    velnor_actions_workflow_renderer::download_plan_step().map_err(|err| {
        OrchestratorError::Contract {
            problem: err.to_string(),
        }
    })
}

/// One always-on matrix-report upload for an obligation's entry.
pub(crate) fn matrix_upload_step(obligation: &CrateObligation) -> Result<Step, OrchestratorError> {
    let name = format!(
        "{} ({})",
        velnor_actions_workflow_renderer::MATRIX_REPORT_UPLOAD_NAME,
        obligation.step_name
    );
    velnor_actions_workflow_renderer::matrix_report_upload_step_for(&obligation.matrix_key, &name)
        .map_err(|err| OrchestratorError::Contract {
            problem: err.to_string(),
        })
}

#[cfg(test)]
#[path = "matrix_step_tests.rs"]
mod matrix_step_tests;
