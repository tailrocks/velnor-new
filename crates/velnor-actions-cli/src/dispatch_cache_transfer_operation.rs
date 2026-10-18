use super::super::roots::{self, CacheRootSet};
use super::engine::validate_clean_absolute;
use super::engine::{export_payload, import_payload};
use super::{
    ACTION_HIT_ENV, ACTION_MATCHED_KEY_ENV, CacheExportProof, CacheRestoreProof,
    CacheRestoreStatus, MAX_ACTION_KEY_BYTES,
};
use std::env;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use velnor_actions_contract::workflow::QualificationCacheBackendObservation;
use velnor_actions_orchestrator::{
    QualificationCacheBindingOutputs, bind_qualification_cache_runtime,
    observe_qualification_cache_backend,
};

#[derive(Debug, Clone, PartialEq, Eq)]
struct RestoreActionObservation {
    hit: bool,
    matched_key: Option<String>,
}

/// Stage a typed cache export and append its bounded evidence outputs.
pub(crate) fn run_export() -> ExitCode {
    match run_export_inner() {
        Ok(body) => append_operation_output(&body, "cache export"),
        Err(problem) => fail_operation("cache export", problem),
    }
}

fn run_export_inner() -> Result<String, &'static str> {
    let inputs =
        super::super::read_inputs(|name| env::var(name).map_err(|_| env::VarError::NotPresent))
            .map_err(|_| "qualification_cache_binding_inputs_invalid")?;
    let request = inputs.request();
    let binding = bind_qualification_cache_runtime(&request)
        .map_err(|_| "qualification_cache_binding_invalid")?;
    let roots = read_bound_roots(&binding, request.runner_temp)?;
    let proof = export_payload(&binding, &roots, request.runner_temp)?;
    export_output_body(&proof)
}

fn export_output_body(proof: &CacheExportProof) -> Result<String, &'static str> {
    let save_requested = proof.save_key.is_some();
    let fields = [
        ("snapshot_admitted", "true".to_owned()),
        ("state_digest", proof.state_digest.clone()),
        ("save_requested", bool_text(save_requested).to_owned()),
        ("save_key", proof.save_key.clone().unwrap_or_default()),
        (
            "save_stage_path",
            proof
                .save_stage_path
                .as_ref()
                .map_or_else(String::new, |path| path.display().to_string()),
        ),
        (
            "manifest_digest",
            proof.manifest_digest.clone().unwrap_or_default(),
        ),
        (
            "export_result",
            if save_requested {
                "staged"
            } else {
                "not_required"
            }
            .to_owned(),
        ),
    ];
    output_body(&fields)
}

/// Admit a restored stage only after backend, manifest, digest, and import checks.
pub(crate) fn run_import() -> ExitCode {
    match run_import_inner() {
        Ok(body) => append_operation_output(&body, "cache import"),
        Err(problem) => fail_operation("cache import", problem),
    }
}

fn run_import_inner() -> Result<String, &'static str> {
    let inputs =
        super::super::read_inputs(|name| env::var(name).map_err(|_| env::VarError::NotPresent))
            .map_err(|_| "qualification_cache_binding_inputs_invalid")?;
    let request = inputs.request();
    let binding = bind_qualification_cache_runtime(&request)
        .map_err(|_| "qualification_cache_binding_invalid")?;
    let roots = read_bound_roots(&binding, request.runner_temp)?;
    let action = read_restore_action(binding.restore_key.is_some())?;
    let observed = if let Some(key) = binding.restore_key.as_deref() {
        let workspace = workspace_path(env::var("GITHUB_WORKSPACE"))?;
        observe_qualification_cache_backend(&workspace, request.repository, key, request.git_ref)
            .map_err(|_| "qualification_cache_backend_unavailable")?
    } else {
        QualificationCacheBackendObservation::NotQueried
    };
    let proof = import_payload(
        &binding,
        &roots,
        request.runner_temp,
        action.hit,
        action.matched_key.as_deref(),
        &observed,
    )?;
    import_output_body(&binding, &action, &observed, &proof)
}

fn read_bound_roots(
    binding: &QualificationCacheBindingOutputs,
    runner_temp: &Path,
) -> Result<CacheRootSet, &'static str> {
    let layer = roots::parse_layer(&binding.layer).ok_or("qualification_cache_roots_invalid")?;
    let text = env::var(roots::ROOTS_ENV).map_err(|_| "qualification_cache_roots_missing")?;
    let roots = CacheRootSet::parse(&text, &binding.matrix_key, layer)?;
    let expected =
        roots::expected_paths_for_environment(layer, runner_temp, |name| env::var(name).ok())?;
    roots.validate_expected_paths(&expected)?;
    Ok(roots)
}

fn read_restore_action(restore_requested: bool) -> Result<RestoreActionObservation, &'static str> {
    let hit_text = env::var(ACTION_HIT_ENV)
        .ok()
        .filter(|value| !value.is_empty());
    let hit = match hit_text.as_deref() {
        Some("true") => true,
        Some("false") => false,
        None if !restore_requested => false,
        _ => return Err("qualification_cache_action_hit_invalid"),
    };
    let matched_key = env::var(ACTION_MATCHED_KEY_ENV)
        .ok()
        .filter(|value| !value.is_empty());
    if matched_key.as_ref().is_some_and(|value| {
        value.len() > MAX_ACTION_KEY_BYTES || !value.bytes().all(|byte| byte.is_ascii_graphic())
    }) {
        return Err("qualification_cache_action_key_invalid");
    }
    Ok(RestoreActionObservation { hit, matched_key })
}

fn workspace_path(value: Result<String, env::VarError>) -> Result<PathBuf, &'static str> {
    let text = value.map_err(|_| "missing_github_workspace")?;
    let path = PathBuf::from(&text);
    validate_clean_absolute(&path).map_err(|_| "github_workspace_path_invalid")?;
    if text.len() > 4_096 {
        return Err("github_workspace_path_invalid");
    }
    Ok(path)
}

fn import_output_body(
    binding: &QualificationCacheBindingOutputs,
    action: &RestoreActionObservation,
    observed: &QualificationCacheBackendObservation,
    proof: &CacheRestoreProof,
) -> Result<String, &'static str> {
    let (status, cache) = match observed {
        QualificationCacheBackendObservation::Absent => ("absent", None),
        QualificationCacheBackendObservation::Found(entry) => ("found", Some(entry)),
        QualificationCacheBackendObservation::NotQueried => ("not_queried", None),
        QualificationCacheBackendObservation::Unavailable => {
            return Err("qualification_cache_backend_unavailable");
        }
    };
    let result = match proof.status {
        CacheRestoreStatus::Disabled => "disabled",
        CacheRestoreStatus::Miss => "miss",
        CacheRestoreStatus::Hit => "hit",
    };
    let mut fields = vec![
        ("restore_result", result.to_owned()),
        ("matrix_key", binding.matrix_key.clone()),
        ("layer", binding.layer.clone()),
        (
            "restore_key",
            binding.restore_key.clone().unwrap_or_default(),
        ),
        ("restore_action_hit", bool_text(action.hit).to_owned()),
        (
            "restore_action_matched_key",
            action.matched_key.clone().unwrap_or_default(),
        ),
        ("backend_status", status.to_owned()),
        (
            "backend_cache_id",
            cache.map_or_else(String::new, |entry| entry.id.to_string()),
        ),
        (
            "backend_cache_key",
            cache.map_or_else(String::new, |entry| entry.key.clone()),
        ),
        (
            "backend_cache_ref",
            cache.map_or_else(String::new, |entry| entry.git_ref.clone()),
        ),
        (
            "backend_cache_version",
            cache.map_or_else(String::new, |entry| entry.version.clone()),
        ),
        (
            "backend_cache_size",
            cache.map_or_else(String::new, |entry| entry.size_bytes.to_string()),
        ),
        (
            "state_digest",
            proof.state_digest.clone().unwrap_or_default(),
        ),
        (
            "restore_stage_path",
            proof
                .stage_path
                .as_ref()
                .map_or_else(String::new, |path| path.display().to_string()),
        ),
    ];
    if proof.status != CacheRestoreStatus::Disabled {
        fields.push(("backend_admitted", "true".to_owned()));
    }
    if proof.status == CacheRestoreStatus::Hit {
        fields.push(("payload_admitted", "true".to_owned()));
        fields.push(("restore_admitted", "true".to_owned()));
    }
    output_body(&fields)
}

fn output_body(fields: &[(&str, String)]) -> Result<String, &'static str> {
    let mut body = String::new();
    for (name, value) in fields {
        if value.chars().any(|ch| matches!(ch, '\n' | '\r')) {
            return Err("qualification_cache_output_invalid");
        }
        body.push_str(name);
        body.push('=');
        body.push_str(value);
        body.push('\n');
    }
    Ok(body)
}

fn append_operation_output(body: &str, operation: &str) -> ExitCode {
    let Some(path) = env::var_os(super::super::GITHUB_OUTPUT_ENV).filter(|value| !value.is_empty())
    else {
        return fail_operation(operation, "missing_github_output");
    };
    match super::super::append_owned_output(&path, body.as_bytes()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(_) => fail_operation(operation, "github_output_write_failed"),
    }
}

fn fail_operation(operation: &str, problem: &str) -> ExitCode {
    eprintln!("velnor-actions: {operation} failed: {problem}");
    ExitCode::from(1)
}

fn bool_text(value: bool) -> &'static str {
    if value { "true" } else { "false" }
}

#[cfg(test)]
#[path = "dispatch_cache_transfer_operation_tests.rs"]
mod tests;
