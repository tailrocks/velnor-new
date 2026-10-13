//! Crate-job step constructors: prelude plus validated obligation env.

use std::collections::BTreeMap;
use std::ffi::OsString;

use crate::OrchestratorError;
use crate::task_report::TASK_ID_ENV;
use velnor_actions_contract::{
    CrateObligation, Stack, Step, StepKind, StepRole, sanitize_error_detail,
};
use velnor_actions_mise::{ISOLATION_ENV, NO_AUTO_INSTALL_ENV, ToolCatalog, ToolHomes};
use velnor_actions_rust::{payload_env_for_kind, step_base_name};
use velnor_actions_workflow_renderer::plan_format::FORMAT_STEP_NAME;

mod tools;

mod tofu_env;

#[cfg(test)]
pub(crate) use tools::crate_needs_tofu_install;
#[cfg(test)]
pub(crate) use tools::task_driver_tools;
pub(crate) use tools::{
    CrateSuite, crate_needs_generate_validators, prepare_crate_tools_step,
    prepare_install_opentofu, suite_for_package,
};

/// `Documentation` obligation step name.
#[cfg(test)]
pub(crate) const DOCUMENTATION_NAME: &str = "Documentation";

/// Env key carrying the obligation's task ID (report lookup key).
pub(crate) const OBLIGATION_TASK_ID_ENV: &str = "VELNOR_TASK_ID";
/// Env key carrying the obligation's task digest.
pub const OBLIGATION_TASK_DIGEST_ENV: &str = "VELNOR_TASK_DIGEST";
/// Env key carrying the obligation's matrix ID.
pub(crate) const OBLIGATION_MATRIX_ID_ENV: &str = "VELNOR_MATRIX_ID";
/// Env key carrying the obligation's matrix key.
pub(crate) const OBLIGATION_MATRIX_KEY_ENV: &str = "VELNOR_MATRIX_KEY";

/// Human step name for one obligation; shards name their index.
///
/// The base name dispatches by the task ID's stack segment: tofu
/// kinds render through the tofu table, everything else through the
/// rust table (unknown segments keep existing behavior).
pub fn step_name_for(kind: &str, task_id: &str) -> String {
    let base = match obligation_stack(task_id) {
        Some(Stack::Tofu) => velnor_actions_tofu_core::step_base_name(kind, FORMAT_STEP_NAME),
        _ => step_base_name(kind, FORMAT_STEP_NAME),
    };
    match shard_suffix(task_id) {
        Some((index, count)) => format!("{base} (shard {index} of {count})"),
        None => base.to_owned(),
    }
}

/// Shard index/count from a trailing `/shard-<index>-of-<count>` segment.
pub fn shard_suffix(task_id: &str) -> Option<(u32, u32)> {
    let (_, index, count) = velnor_actions_contract::split_shard_suffix(task_id)?;
    Some((index, count))
}

/// Known stack for one obligation task ID, if its segment parses.
fn obligation_stack(task_id: &str) -> Option<Stack> {
    velnor_actions_orchestrator_core::extension_schemas::task_stack_segment(task_id)
        .and_then(Stack::from_id)
}

/// Fixed payload env for one obligation, dispatched by stack.
///
/// Tofu kinds thread through the tofu env mapping (the T03
/// automation pair); everything else keeps the rust mapping.
fn payload_env_for_obligation(task_id: &str, kind: &str) -> Vec<(OsString, OsString)> {
    match obligation_stack(task_id) {
        Some(Stack::Tofu) => velnor_actions_tofu_core::payload_env_for_kind(kind),
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
pub fn task_step_env(
    catalog: &ToolCatalog,
    extra: &BTreeMap<String, String>,
    needs_rust: bool,
) -> Result<BTreeMap<String, String>, OrchestratorError> {
    use velnor_actions_workflow_steps::toolchain_env;
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
        base.insert(
            velnor_actions_tofu_core::TF_DATA_DIR_ENV.to_owned(),
            data_dir,
        );
    }
    if let Some(cache_dir) = tofu_env::tofu_plugin_cache_dir_for_extra(extra)? {
        base.insert(
            velnor_actions_tofu_core::TF_PLUGIN_CACHE_DIR_ENV.to_owned(),
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
            velnor_actions_workflow_jobs::context::PLAN_JOB_ID.to_owned(),
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
/// helper's [`crate::task_report::REPORT_OP`] with the exit code, then exits with the
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
pub fn obligation_step(
    obligation: &CrateObligation,
    catalog: &ToolCatalog,
    _downstream: &[String],
    matrix_cap: Option<u32>,
    helper_version: &str,
) -> Result<Step, OrchestratorError> {
    // Unknown segments keep the previous single-stack behavior: the
    // grammar validation below fails them as malformed task IDs.
    let stack_id = obligation_stack(&obligation.task_id).map_or(Stack::Rust.id(), Stack::id);
    let matrix_id =
        velnor_actions_contract::matrix_id_for_task_group(stack_id, &obligation.task_id)
            .map_err(crate::internal::internal_contract)?;
    let needs_rust = obligation_stack(&obligation.task_id) != Some(Stack::Tofu);
    let mut payload_env = BTreeMap::new();
    for (key, value) in payload_env_for_obligation(&obligation.task_id, &obligation.kind) {
        payload_env.insert(
            key.to_string_lossy().into_owned(),
            value.to_string_lossy().into_owned(),
        );
    }
    let mut step = if needs_rust {
        let env = task_step_env(catalog, &payload_env, true)?;
        Step {
            name: obligation.step_name.clone(),
            id: None,
            role: None,
            condition: None,
            kind: StepKind::TaskExecution {
                argv: obligation.run.clone(),
                env,
                task_id: obligation.task_id.clone(),
                task_digest: obligation.task_digest.clone(),
                toolchain_inputs: obligation.toolchain_inputs.clone(),
                matrix_id,
                matrix_key: obligation.matrix_key.clone(),
                report_helper_version: helper_version.to_owned(),
                matrix_max_parallel: matrix_cap,
            },
        }
    } else {
        let mut identity = obligation_identity_env(
            &obligation.task_id,
            &obligation.task_digest,
            &matrix_id,
            &obligation.matrix_key,
            matrix_cap,
        );
        identity.extend(payload_env);
        check_identity_env_contract(&identity, &obligation.task_id)?;
        let env = task_step_env(catalog, &identity, false)?;
        let joined = velnor_actions_workflow_renderer::join_argv_for_run(&obligation.run).map_err(
            |err| OrchestratorError::Contract {
                problem: err.to_string(),
            },
        )?;
        let run = report_wrapper_argv(&joined, &helper_path_for_version());
        let mut tofu =
            velnor_actions_workflow_renderer::shell_step(&obligation.step_name, run, env)
                .map_err(OrchestratorError::from)?;
        tofu.role = Some(StepRole::TofuProviderUse);
        tofu
    };
    // Skip when the plan covered this obligation: unknown coverage
    // (absent output) executes, so the gate can only skip proven work.
    //
    // Task IDs reaching here validated at obligation construction, and
    // the grammar admits no quotes or commas, so the generated
    // expression cannot break out of its string literal.
    step.condition = Some(skip_condition(&obligation.task_id)?);
    Ok(step)
}

/// Generated `if:` gate skipping `task_id` when the plan covered it.
///
/// Evaluates over the plan job's `covered_tasks` output, which the
/// crate job reads through `needs.plan`; an absent or empty output
/// matches nothing, so unknown coverage always executes.
/// # Errors
///
/// Returns a contract error when the task ID is malformed.
fn skip_condition(task_id: &str) -> Result<String, OrchestratorError> {
    use velnor_actions_contract::validate_task_id;
    use velnor_actions_orchestrator_core::internal_contract;
    use velnor_actions_workflow_renderer::COVERED_TASKS_OUTPUT;
    validate_task_id(task_id).map_err(internal_contract)?;
    Ok(format!(
        "!contains(needs.plan.outputs.{COVERED_TASKS_OUTPUT}, ',{task_id},')"
    ))
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

#[path = "matrix_step_reports.rs"]
mod matrix_step_reports;
pub(crate) use matrix_step_reports::{
    crate_upload_step, deferred_report_argv, download_plan_step, helper_path_for_version,
    outcome_path_for_key, outcome_wrapper_argv, report_wrapper_argv, start_path_for_key,
};

/// Every workspace member: package name plus member directory.
///
/// The `crates/` group walk is the discovery, not a frozen list: new
/// groups and packages join automatically.
#[cfg(test)]
pub(crate) fn workspace_suites(root: &std::path::Path) -> Vec<(String, std::path::PathBuf)> {
    let mut members = Vec::new();
    let crates = std::fs::read_dir(root.join("crates")).expect("crates dir lists");
    for entry in crates {
        let group = entry.expect("dir entry reads").path();
        if !group.is_dir() {
            continue;
        }
        let packages = std::fs::read_dir(&group).expect("group dir lists");
        for package in packages {
            let dir = package.expect("package entry reads").path();
            let manifest = dir.join("Cargo.toml");
            if !manifest.is_file() {
                continue;
            }
            let text = std::fs::read_to_string(&manifest).expect("member manifest reads");
            let mut in_package = false;
            for line in text.lines() {
                if line.trim() == "[package]" {
                    in_package = true;
                } else if line.starts_with('[') {
                    in_package = false;
                } else if in_package && line.trim_start().starts_with("name = ") {
                    let name = line
                        .trim_start()
                        .trim_start_matches("name = ")
                        .trim()
                        .trim_matches('"');
                    members.push((name.to_owned(), dir.clone()));
                    break;
                }
            }
        }
    }
    members.sort();
    members
}

/// Suite sources: integration `tests/` plus unit `tests.rs` files.
///
/// Embedded `src/**/tests/*.rs` harnesses (including the caller's own
/// classification test) are NOT suite sources, so marker vocabulary in
/// test scaffolding never trips the scan.
#[cfg(test)]
fn suite_files(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let mut pending = vec![dir.join("tests")];
    while let Some(path) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&path) else {
            continue;
        };
        for entry in entries {
            let entry = entry.expect("suite entry reads");
            if entry.file_type().expect("suite entry types").is_dir() {
                pending.push(entry.path());
            } else if entry.path().extension().is_some_and(|ext| ext == "rs") {
                out.push(entry.path());
            }
        }
    }
    let mut src_pending = vec![dir.join("src")];
    while let Some(path) = src_pending.pop() {
        let Ok(entries) = std::fs::read_dir(&path) else {
            continue;
        };
        for entry in entries {
            let entry = entry.expect("src entry reads");
            if entry.file_type().expect("src entry types").is_dir() {
                src_pending.push(entry.path());
            } else if entry
                .path()
                .file_name()
                .is_some_and(|name| name == "tests.rs")
            {
                out.push(entry.path());
            }
        }
    }
    out.sort();
    out
}

/// True when any suite file contains any marker.
#[cfg(test)]
pub(crate) fn suite_has_marker(dir: &std::path::Path, markers: &[&str]) -> bool {
    suite_files(dir).iter().any(|path| {
        let text = std::fs::read_to_string(path).expect("suite file reads");
        markers.iter().any(|marker| text.contains(marker))
    })
}

/// True when one suite file contains the primary plus any secondary.
///
/// Same-file pairing distinguishes real execution (`tofu_exec`
/// plus a `run_*` call) from ctor-only and env-assertion uses.
#[cfg(test)]
pub(crate) fn suite_has_marker_pair(
    dir: &std::path::Path,
    primary: &str,
    secondaries: &[&str],
) -> bool {
    suite_files(dir).iter().any(|path| {
        let text = std::fs::read_to_string(path).expect("suite file reads");
        text.contains(primary) && secondaries.iter().any(|marker| text.contains(marker))
    })
}

#[cfg(test)]
mod tests;
