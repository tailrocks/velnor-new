//! Fixed owner-scoped runtime and root observations for qualification cache.

use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

use velnor_actions_orchestrator::{
    QUALIFICATION_CACHE_ROOTS_OP, QualificationCacheRuntimePhase,
    observe_qualification_cache_runtime, observe_qualification_cache_runtime_after_import,
    write_qualification_cache_roots,
};

use super::{cache_bind, fail_internal};

pub(crate) const ROOTS_OP: &str = QUALIFICATION_CACHE_ROOTS_OP;
pub(crate) const RUNTIME_OP: &str = "observe-qualification-cache-runtime-v1";
const RUNNER_TEMP: &str = "RUNNER_TEMP";
const OWNER_ID: &str = "VELNOR_QUALIFICATION_CACHE_OWNER_ID";
const LAYER: &str = "VELNOR_QUALIFICATION_CACHE_LAYER";
const RUNTIME_PHASE: &str = "VELNOR_QUALIFICATION_CACHE_RUNTIME_PHASE";

pub(crate) fn observe_roots() -> ExitCode {
    let Some(runner_temp) = env::var_os(RUNNER_TEMP).filter(|value| !value.is_empty()) else {
        return fail_internal("missing_runner_temp");
    };
    let Some(owner_id) = env::var_os(OWNER_ID).and_then(|value| value.into_string().ok()) else {
        return fail_internal("missing_qualification_cache_owner_id");
    };
    let Some(layer) = env::var_os(LAYER).and_then(|value| value.into_string().ok()) else {
        return fail_internal("missing_qualification_cache_layer");
    };
    let run_id = match decimal_env("GITHUB_RUN_ID") {
        Ok(value) => value,
        Err(problem) => return fail_internal(problem),
    };
    let attempt = match decimal_env("GITHUB_RUN_ATTEMPT") {
        Ok(value) => value,
        Err(problem) => return fail_internal(problem),
    };
    let Ok(attempt) = u32::try_from(attempt) else {
        return fail_internal("bad_run_coordinates");
    };
    let path = match write_qualification_cache_roots(
        PathBuf::from(runner_temp).as_path(),
        run_id,
        attempt,
        &owner_id,
        &layer,
        |name| env::var(name).ok().filter(|value| !value.is_empty()),
    ) {
        Ok(path) => path,
        Err(error) => return fail_internal(&error.to_string()),
    };
    let Some(output) = env::var_os(cache_bind::GITHUB_OUTPUT_ENV).filter(|value| !value.is_empty())
    else {
        return fail_internal("missing_github_output");
    };
    let body = format!("root_observation_path={path}\n");
    match cache_bind::append_owned_output(&output, body.as_bytes()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => fail_internal(&format!("append_github_output:{error}")),
    }
}

pub(crate) fn observe_runtime() -> ExitCode {
    let inputs = match cache_bind::read_inputs(|name| env::var(name)) {
        Ok(inputs) => inputs,
        Err(problem) => return fail_internal(&problem),
    };
    let request = inputs.request();
    let phase = match read_runtime_phase(|name| env::var(name).ok()) {
        Ok(phase) => phase,
        Err(problem) => return fail_internal(problem),
    };
    let (path, matches) = match phase {
        QualificationCacheRuntimePhase::BeforeImport => {
            match observe_qualification_cache_runtime(&request, |name| {
                env::var(name).ok().filter(|value| !value.is_empty())
            }) {
                Ok(path) => (path, true),
                Err(error) => return fail_internal(&error.to_string()),
            }
        }
        QualificationCacheRuntimePhase::AfterImport => {
            match observe_qualification_cache_runtime_after_import(&request, |name| {
                env::var(name).ok().filter(|value| !value.is_empty())
            }) {
                Ok(result) => result,
                Err(error) => return fail_internal(&error.to_string()),
            }
        }
    };
    let Some(output) = env::var_os(cache_bind::GITHUB_OUTPUT_ENV).filter(|value| !value.is_empty())
    else {
        return fail_internal("missing_github_output");
    };
    let body = format!(
        "runtime_identity_path={path}\nruntime_identity_matches={}\n",
        if matches { "true" } else { "false" }
    );
    match cache_bind::append_owned_output(&output, body.as_bytes()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => fail_internal(&format!("append_github_output:{error}")),
    }
}

fn read_runtime_phase(
    get: impl FnOnce(&str) -> Option<String>,
) -> Result<QualificationCacheRuntimePhase, &'static str> {
    let value = get(RUNTIME_PHASE).ok_or("qualification_cache_runtime_phase_invalid")?;
    QualificationCacheRuntimePhase::parse(&value)
}

#[cfg(test)]
#[path = "dispatch_cache_runtime_tests.rs"]
mod tests;

fn decimal_env(name: &str) -> Result<u64, &'static str> {
    let text = env::var(name).map_err(|_| "missing_run_coordinates")?;
    let parsed = text.parse::<u64>().map_err(|_| "bad_run_coordinates")?;
    if parsed == 0 || parsed.to_string() != text {
        return Err("bad_run_coordinates");
    }
    Ok(parsed)
}
