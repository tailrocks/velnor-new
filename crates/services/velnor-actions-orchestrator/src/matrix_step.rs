//! Crate-job step constructors: prelude plus validated obligation env.

use std::collections::BTreeMap;
use std::ffi::OsString;

use velnor_actions_contract::{CrateObligation, Stack, Step, StepRole, sanitize_error_detail};
use velnor_actions_mise::{ISOLATION_ENV, NO_AUTO_INSTALL_ENV, ToolCatalog, ToolHomes};
use velnor_actions_rust::{payload_env_for_kind, step_base_name};
use velnor_actions_workflow_renderer::plan_format::FORMAT_STEP_NAME;
use velnor_actions_workflow_renderer::steps::{INTERNAL_OP_ENV, STAGED_BINARY_PREFIX};

use crate::OrchestratorError;
use crate::task_report::{EXIT_CODE_ENV, REPORT_OP, START_MS_ENV, TASK_ID_ENV};

#[path = "matrix_tools.rs"]
mod tools;

#[path = "matrix_tofu_env.rs"]
mod tofu_env;

#[cfg(test)]
pub(crate) use tools::crate_needs_tofu_install;
#[cfg(test)]
pub(crate) use tools::task_driver_tools;
pub(crate) use tools::{
    crate_needs_generate_validators, prepare_crate_tools_step, prepare_install_opentofu,
};

/// `Documentation` obligation step name.
#[cfg(test)]
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
///
/// The base name dispatches by the task ID's stack segment: tofu
/// kinds render through the tofu table, everything else through the
/// rust table (unknown segments keep existing behavior).
pub(crate) fn step_name_for(kind: &str, task_id: &str) -> String {
    let base = match obligation_stack(task_id) {
        Some(Stack::Tofu) => velnor_actions_tofu::step_base_name(kind, FORMAT_STEP_NAME),
        _ => step_base_name(kind, FORMAT_STEP_NAME),
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

/// Known stack for one obligation task ID, if its segment parses.
fn obligation_stack(task_id: &str) -> Option<Stack> {
    crate::extension_schemas::task_stack_segment(task_id).and_then(Stack::from_id)
}

/// Fixed payload env for one obligation, dispatched by stack.
///
/// Tofu kinds thread through the tofu env mapping (the T03
/// automation pair); everything else keeps the rust mapping.
fn payload_env_for_obligation(task_id: &str, kind: &str) -> Vec<(OsString, OsString)> {
    match obligation_stack(task_id) {
        Some(Stack::Tofu) => velnor_actions_tofu::payload_env_for_kind(kind),
        _ => payload_env_for_kind(kind),
    }
}

/// Validated env every crate-job step runs with.
///
/// Single constructor shared by obligation steps and `Fetch Cargo
/// sources`: the isolation quartet plus install disable from the Mise
/// adapter's single source, the owned-homes triple for rust steps,
/// and caller extras. Tofu obligations carry no triple; they carry
/// the isolated per-root `TF_DATA_DIR` from the tofu adapter's
/// derivation instead (the temp CLI config path stays local-only
/// until a materialization step lands). Reserved keys in the extras
/// fail closed, so generated steps use the same validated contract as
/// local helper requests and fetch plus consumers can never drift
/// apart (run 36560676954 failed every leg when only `Run task`
/// carried the triple).
///
/// # Errors
///
/// Returns a contract error for reserved extras (triple keys included
/// when `needs_rust` is false) and a render error for blank triple
/// inputs or denied credential keys.
pub(crate) fn task_step_env(
    catalog: &ToolCatalog,
    extra: &BTreeMap<String, String>,
    needs_rust: bool,
) -> Result<BTreeMap<String, String>, OrchestratorError> {
    use velnor_actions_workflow_renderer::toolchain_env;
    for key in extra.keys() {
        if velnor_actions_mise::command::is_reserved_env_key(key)
            || (!needs_rust && toolchain_env::TOOLCHAIN_HOME_KEYS.contains(&key.as_str()))
        {
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
    if let Some(data_dir) = tofu_env::tofu_data_dir_for_extra(extra)? {
        base.insert(velnor_actions_tofu::TF_DATA_DIR_ENV.to_owned(), data_dir);
    }
    if let Some(cache_dir) = tofu_env::tofu_plugin_cache_dir_for_extra(extra)? {
        base.insert(
            velnor_actions_tofu::TF_PLUGIN_CACHE_DIR_ENV.to_owned(),
            cache_dir,
        );
    }
    if !needs_rust {
        toolchain_env::reject_denied_step_keys(&base).map_err(OrchestratorError::from)?;
        return Ok(base);
    }
    let homes = ToolHomes::runner_temp();
    let toolchain = catalog.rustup_toolchain();
    toolchain_env::checked_task_env(&base, homes.rustup_home(), homes.cargo_home(), &toolchain)
        .map_err(OrchestratorError::from)
}

/// Fixed obligation identity env carried by every obligation step.
///
/// Binds the step to its task, digest, and matrix coordinates with
/// fixed values (never `${{ }}` expressions), so the report wrapper
/// invokes the helper for the right obligation and merge locates its
/// evidence. Downstream same-job task IDs ride along so a failure
/// reports its skipped successors; empty downstream stays absent. A
/// root-job cap additionally declares the matrix marker trio (the
/// plan producer every obligation already consumes, plus the bound),
/// which the renderer turns into `strategy.max-parallel`.
pub(crate) fn obligation_identity_env(
    task_id: &str,
    task_digest: &str,
    matrix_id: &str,
    matrix_key: &str,
    matrix_cap: Option<u32>,
) -> BTreeMap<String, String> {
    let mut identity = BTreeMap::from([
        (OBLIGATION_TASK_ID_ENV.to_owned(), task_id.to_owned()),
        (
            OBLIGATION_TASK_DIGEST_ENV.to_owned(),
            task_digest.to_owned(),
        ),
        (OBLIGATION_MATRIX_ID_ENV.to_owned(), matrix_id.to_owned()),
        (OBLIGATION_MATRIX_KEY_ENV.to_owned(), matrix_key.to_owned()),
    ]);
    if let Some(cap) = matrix_cap {
        identity.insert(
            velnor_actions_workflow_renderer::MATRIX_NEEDS_JOB_ENV.to_owned(),
            velnor_actions_workflow_renderer::render::PLAN_JOB_ID.to_owned(),
        );
        identity.insert(
            velnor_actions_workflow_renderer::MATRIX_OUTPUT_ENV.to_owned(),
            velnor_actions_workflow_renderer::COVERED_TASKS_OUTPUT.to_owned(),
        );
        identity.insert(
            velnor_actions_workflow_renderer::MATRIX_MAX_PARALLEL_ENV.to_owned(),
            cap.to_string(),
        );
    }
    identity
}

/// One obligation shell step: fixed argv wrapped with report capture.
///
/// The wrapper runs the obligation, captures `$?`, invokes the staged
/// helper's [`REPORT_OP`] with the exit code, then exits with the
/// obligation's own code (a report failure surfaces only when the
/// obligation itself passed, so failures never mask each other).
/// Identity env doubles as the report lookup key; the plan binds the
/// digests, never these baked values. Doc obligations additionally
/// carry the adapter `payload_env_for_kind` pairs (`RUSTDOCFLAGS=-D
/// warnings`) and tofu obligations the automation pair, matching the
/// plan identity envelope. Tofu obligations run triple-less; every
/// other stack keeps the owned-homes triple. The step skips via `if:`
/// when the plan covered this obligation; unknown coverage executes.
///
/// # Errors
///
/// Returns contract errors for invalid argv, joins, wrapper shapes, or
/// task IDs unfit for the generated skip gate.
pub(crate) fn obligation_step(
    obligation: &CrateObligation,
    catalog: &ToolCatalog,
    _downstream: &[String],
    matrix_cap: Option<u32>,
) -> Result<Step, OrchestratorError> {
    // Unknown segments keep the previous single-stack behavior: the
    // grammar validation below fails them as malformed task IDs.
    let stack_id = obligation_stack(&obligation.task_id).map_or(Stack::Rust.id(), Stack::id);
    let matrix_id =
        velnor_actions_contract::matrix_id_for_task_group(stack_id, &obligation.task_id)
            .map_err(crate::internal::internal_contract)?;
    let mut identity = obligation_identity_env(
        &obligation.task_id,
        &obligation.task_digest,
        &matrix_id,
        &obligation.matrix_key,
        matrix_cap,
    );
    for (key, value) in payload_env_for_obligation(&obligation.task_id, &obligation.kind) {
        identity.insert(
            key.to_string_lossy().into_owned(),
            value.to_string_lossy().into_owned(),
        );
    }
    check_identity_env_contract(&identity, &obligation.task_id)?;
    let needs_rust = obligation_stack(&obligation.task_id) != Some(Stack::Tofu);
    let env = task_step_env(catalog, &identity, needs_rust)?;
    let joined =
        velnor_actions_workflow_renderer::join_argv_for_run(&obligation.run).map_err(|err| {
            OrchestratorError::Contract {
                problem: err.to_string(),
            }
        })?;
    let run = report_wrapper_argv(&joined, &helper_path_for_version());
    let mut step = velnor_actions_workflow_renderer::shell_step(&obligation.step_name, run, env)
        .map_err(OrchestratorError::from)?;
    if !needs_rust {
        step.role = Some(StepRole::TofuProviderUse);
    }
    // Skip when the plan covered this obligation: unknown coverage
    // (absent output) executes, so the gate can only skip proven work.
    //
    // Task IDs reaching here validated at obligation construction, and
    // the grammar admits no quotes or commas, so the generated
    // expression cannot break out of its string literal.
    step.condition = Some(crate::covered_tasks::skip_condition(&obligation.task_id)?);
    Ok(step)
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
/// Captures the wall-clock start with GNU `date`'s millisecond format,
/// runs the obligation, captures `$?`, reports through the staged
/// helper's [`REPORT_OP`], then exits with the obligation code (helper
/// failure surfaces only on an otherwise passing obligation, so failures
/// never mask each other). This wrapper is used only by generic jobs,
/// whose workflow runner labels are Ubuntu; named checks invoke the helper
/// directly and measure duration with Rust `Instant` on Linux or macOS.
/// Credential removal is the step constructor's job (`shell_step`
/// prefixes argv-wide `env -u`), not a script prelude's: obligations
/// execute repository code (build scripts), and the step env cannot
/// shadow runner-injected credentials (D3).
pub(crate) fn report_wrapper_argv(joined: &str, helper: &str) -> Vec<String> {
    vec![
        "sh".to_owned(),
        "-c".to_owned(),
        format!(
            "s=$(date +%s%3N); {joined}; code=$?; {EXIT_CODE_ENV}=\"$code\" {START_MS_ENV}=\"$s\" {INTERNAL_OP_ENV}={REPORT_OP} \"{helper}\"; helper_code=$?; if [ \"$code\" -ne 0 ]; then exit \"$code\"; fi; exit \"$helper_code\""
        ),
    ]
}

/// `sh -c` argv saving one joined command's exit to an outcome file.
///
/// Two-phase shape for the plan-job workspace Format: the plan does
/// not exist yet at format time, so the wrapper records `$?` plus the
/// wall-clock start stamp, and a post-plan step reports through
/// [`deferred_report_argv`].
pub(crate) fn outcome_wrapper_argv(
    joined: &str,
    outcome_path: &str,
    start_path: &str,
) -> Vec<String> {
    vec![
        "sh".to_owned(),
        "-c".to_owned(),
        format!(
            "date +%s%3N > \"{start_path}\"; {joined}; code=$?; echo \"$code\" > \"{outcome_path}\"; exit \"$code\""
        ),
    ]
}

/// `sh -c` argv reporting one saved outcome through the staged helper.
///
/// Reads the exit code and start stamp the outcome wrapper saved (a
/// missing file leaves the value empty and the helper fails closed),
/// then invokes [`REPORT_OP`]; the step exits with the helper's code.
pub(crate) fn deferred_report_argv(
    outcome_path: &str,
    helper: &str,
    start_path: &str,
) -> Vec<String> {
    vec![
        "sh".to_owned(),
        "-c".to_owned(),
        format!(
            "read -r code rest < \"{outcome_path}\"; read -r start_ms rest < \"{start_path}\"; {EXIT_CODE_ENV}=\"$code\" {START_MS_ENV}=\"$start_ms\" {INTERNAL_OP_ENV}={REPORT_OP} \"{helper}\""
        ),
    ]
}

/// Shell-spelled outcome file for one matrix key under runner temp.
pub(crate) fn outcome_path_for_key(matrix_key: &str) -> String {
    format!("$RUNNER_TEMP/velnor/outcome-{matrix_key}")
}

/// Shell-spelled start-stamp file for one matrix key under runner temp.
pub(crate) fn start_path_for_key(matrix_key: &str) -> String {
    format!("$RUNNER_TEMP/velnor/start-{matrix_key}")
}

/// Plan-artifact download: report wrappers resolve identities from it.
pub(crate) fn download_plan_step() -> Result<Step, OrchestratorError> {
    velnor_actions_workflow_renderer::download_plan_step().map_err(|err| {
        OrchestratorError::Contract {
            problem: err.to_string(),
        }
    })
}

/// One always-on crate-report upload carrying a job's every entry.
pub(crate) fn crate_upload_step(job_id: &str) -> Result<Step, OrchestratorError> {
    velnor_actions_workflow_renderer::crate_job_report_upload_step(job_id).map_err(|err| {
        OrchestratorError::Contract {
            problem: err.to_string(),
        }
    })
}

#[cfg(test)]
#[path = "matrix_step_tests.rs"]
mod matrix_step_tests;

#[cfg(test)]
#[path = "matrix_step_tofu_tests.rs"]
mod matrix_step_tofu_tests;

#[cfg(test)]
#[path = "matrix_step_tofu_install_tests.rs"]
mod matrix_step_tofu_install_tests;
