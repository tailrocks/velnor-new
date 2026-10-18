use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

use velnor_actions_orchestrator::{
    CacheActionObservation, CacheRestoreStatus, QualificationCacheBackendObservation,
    QualificationCacheBindingRequest, QualificationCachePayloadContext,
    QualificationCachePayloadExport, QualificationCachePayloadImport,
    QualificationCacheRootEnvironment, bind_qualification_cache_runtime,
    export_qualification_cache_payload, import_qualification_cache_payload,
    validate_qualification_cache_root_observation,
};

const ROOTS_ENV: &str = "VELNOR_QUALIFICATION_CACHE_ROOTS";
const ACTION_HIT_ENV: &str = "VELNOR_QUALIFICATION_CACHE_HIT";
const ACTION_MATCHED_KEY_ENV: &str = "VELNOR_QUALIFICATION_CACHE_MATCHED_KEY";

#[derive(Debug, Default)]
struct RootPaths {
    mbx_cache_dir: Option<PathBuf>,
    cargo_home: Option<PathBuf>,
    mise_cargo_home: Option<PathBuf>,
    mise_data_dir: Option<PathBuf>,
    rustup_home: Option<PathBuf>,
    mise_rustup_home: Option<PathBuf>,
    tofu_plugin_cache_dir: Option<PathBuf>,
    mise_task_cache_dir: Option<PathBuf>,
}

impl RootPaths {
    fn from_environment() -> Self {
        Self {
            mbx_cache_dir: read_path("MBX_CACHE_DIR"),
            cargo_home: read_path("CARGO_HOME"),
            mise_cargo_home: read_path("MISE_CARGO_HOME"),
            mise_data_dir: read_path("MISE_DATA_DIR"),
            rustup_home: read_path("RUSTUP_HOME"),
            mise_rustup_home: read_path("MISE_RUSTUP_HOME"),
            tofu_plugin_cache_dir: read_path("TF_PLUGIN_CACHE_DIR"),
            mise_task_cache_dir: read_path("MISE_TASK_CACHE_DIR"),
        }
    }

    fn observation(&self) -> QualificationCacheRootEnvironment<'_> {
        QualificationCacheRootEnvironment {
            mbx_cache_dir: self.mbx_cache_dir.as_deref(),
            cargo_home: self.cargo_home.as_deref(),
            mise_cargo_home: self.mise_cargo_home.as_deref(),
            mise_data_dir: self.mise_data_dir.as_deref(),
            rustup_home: self.rustup_home.as_deref(),
            mise_rustup_home: self.mise_rustup_home.as_deref(),
            tofu_plugin_cache_dir: self.tofu_plugin_cache_dir.as_deref(),
            mise_task_cache_dir: self.mise_task_cache_dir.as_deref(),
        }
    }
}

pub(super) fn validate_root_observation(
    request: &QualificationCacheBindingRequest<'_>,
    roots_json: &str,
) -> Result<(), &'static str> {
    let roots = RootPaths::from_environment();
    validate_qualification_cache_root_observation(request, roots_json, roots.observation())
}

pub(crate) fn run_export() -> ExitCode {
    match run_export_inner() {
        Ok(body) => append_operation_output(&body, "cache export"),
        Err(problem) => fail_operation("cache export", problem),
    }
}

fn run_export_inner() -> Result<String, &'static str> {
    let inputs = read_binding_inputs()?;
    let request = inputs.request();
    let roots_json = read_roots_json()?;
    let root_paths = RootPaths::from_environment();
    let context = QualificationCachePayloadContext {
        binding: &request,
        roots_json: &roots_json,
        root_environment: root_paths.observation(),
    };
    let result = export_qualification_cache_payload(&context)?;
    export_output_body(&result)
}

fn export_output_body(result: &QualificationCachePayloadExport) -> Result<String, &'static str> {
    let proof = &result.proof;
    let save_requested = proof.save_key().is_some();
    let fields = [
        ("snapshot_admitted", "true".to_owned()),
        ("state_digest", proof.state_digest().to_owned()),
        ("save_requested", bool_text(save_requested).to_owned()),
        ("save_key", proof.save_key().unwrap_or_default().to_owned()),
        (
            "save_stage_path",
            proof
                .save_stage_path()
                .map_or_else(String::new, |path| path.display().to_string()),
        ),
        (
            "manifest_digest",
            proof.manifest_digest().unwrap_or_default().to_owned(),
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

pub(crate) fn run_import() -> ExitCode {
    match run_import_inner() {
        Ok(body) => append_operation_output(&body, "cache import"),
        Err(problem) => fail_operation("cache import", problem),
    }
}

fn run_import_inner() -> Result<String, &'static str> {
    let inputs = read_binding_inputs()?;
    let request = inputs.request();
    let binding = bind_qualification_cache_runtime(&request)
        .map_err(|_| "qualification_cache_binding_invalid")?;
    let roots_json = read_roots_json()?;
    let root_paths = RootPaths::from_environment();
    let context = QualificationCachePayloadContext {
        binding: &request,
        roots_json: &roots_json,
        root_environment: root_paths.observation(),
    };
    let action = read_restore_action(binding.restore_key.is_some())?;
    let result = import_qualification_cache_payload(&context, &action)?;
    import_output_body(&result, &action)
}

fn import_output_body(
    result: &QualificationCachePayloadImport,
    action: &CacheActionObservation,
) -> Result<String, &'static str> {
    let (status, cache) = match &result.backend {
        QualificationCacheBackendObservation::Absent => ("absent", None),
        QualificationCacheBackendObservation::Found(entry) => ("found", Some(entry)),
        QualificationCacheBackendObservation::NotQueried => ("not_queried", None),
        QualificationCacheBackendObservation::Unavailable => {
            return Err("qualification_cache_backend_unavailable");
        }
    };
    let proof = &result.proof;
    let restore_result = match proof.status() {
        CacheRestoreStatus::Disabled => "disabled",
        CacheRestoreStatus::Miss => "miss",
        CacheRestoreStatus::Hit => "hit",
    };
    let binding = &result.binding;
    let fields = vec![
        ("restore_result", restore_result.to_owned()),
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
            proof.state_digest().unwrap_or_default().to_owned(),
        ),
        (
            "restore_stage_path",
            proof
                .stage_path()
                .map_or_else(String::new, |path| path.display().to_string()),
        ),
    ];
    let mut fields = fields;
    if proof.status() != CacheRestoreStatus::Disabled {
        fields.push(("backend_admitted", "true".to_owned()));
    }
    if proof.status() == CacheRestoreStatus::Hit {
        fields.push(("payload_admitted", "true".to_owned()));
        fields.push(("restore_admitted", "true".to_owned()));
    }
    output_body(&fields)
}

fn read_binding_inputs() -> Result<super::super::BindInputs, &'static str> {
    super::super::read_inputs(|name| env::var(name))
        .map_err(|_| "qualification_cache_binding_inputs_invalid")
}

fn read_roots_json() -> Result<String, &'static str> {
    env::var(ROOTS_ENV).map_err(|_| "qualification_cache_roots_missing")
}

fn read_restore_action(restore_requested: bool) -> Result<CacheActionObservation, &'static str> {
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
    Ok(CacheActionObservation { hit, matched_key })
}

fn read_path(name: &str) -> Option<PathBuf> {
    env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
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

fn bool_text(value: bool) -> &'static str {
    if value { "true" } else { "false" }
}

fn fail_operation(operation: &str, problem: &str) -> ExitCode {
    eprintln!("velnor-actions: {operation} failed: {problem}");
    ExitCode::from(1)
}
