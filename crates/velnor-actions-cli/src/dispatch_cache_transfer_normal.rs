use std::env;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use velnor_actions_orchestrator::{
    CacheActionObservation, CacheExportProof, CacheRestoreProof, CacheRestoreStatus,
    NormalToolsCachePayloadRequest, export_normal_tools_cache_payload,
    import_normal_tools_cache_payload, normal_tools_cache_stage_path,
};

pub(crate) const EXPORT_OP: &str = "export-normal-tools-cache-payload-v1";
pub(crate) const IMPORT_OP: &str = "import-normal-tools-cache-payload-v1";

const KEY_ENV: &str = "VELNOR_NORMAL_CACHE_KEY";
const IDENTITY_ENV: &str = "VELNOR_NORMAL_CACHE_IDENTITY";
const ENABLED_ENV: &str = "VELNOR_NORMAL_CACHE_ENABLED";
const MISE_DATA_ENV: &str = "VELNOR_NORMAL_CACHE_MISE_DATA_DIR";
const CARGO_HOME_ENV: &str = "VELNOR_NORMAL_CACHE_CARGO_HOME";
const RUSTUP_HOME_ENV: &str = "VELNOR_NORMAL_CACHE_RUSTUP_HOME";
const STAGE_PATH_ENV: &str = "VELNOR_NORMAL_CACHE_STAGE_PATH";
const HIT_ENV: &str = "VELNOR_NORMAL_CACHE_HIT";
const MATCHED_KEY_ENV: &str = "VELNOR_NORMAL_CACHE_MATCHED_KEY";

#[derive(Debug)]
struct Inputs {
    runner_temp: PathBuf,
    home: PathBuf,
    identity: String,
    cache_key: String,
    mise_data_dir: PathBuf,
    cargo_home: PathBuf,
    rustup_home: PathBuf,
    stage_path: PathBuf,
    action: Option<CacheActionObservation>,
}

pub(crate) fn run_export() -> ExitCode {
    match read_inputs(false).and_then(|inputs| export(&inputs)) {
        Ok(body) => append_output(&body, "normal cache export"),
        Err(problem) => fail("normal cache export", problem),
    }
}

pub(crate) fn run_import() -> ExitCode {
    match read_inputs(true).and_then(|inputs| import(&inputs)) {
        Ok(body) => append_output(&body, "normal cache import"),
        Err(problem) => fail("normal cache import", problem),
    }
}

fn export(inputs: &Inputs) -> Result<String, &'static str> {
    let request = payload_request(inputs);
    let proof = export_normal_tools_cache_payload(&request)?;
    export_output(&proof)
}

fn import(inputs: &Inputs) -> Result<String, &'static str> {
    let request = payload_request(inputs);
    let action = inputs
        .action
        .as_ref()
        .ok_or("normal_cache_action_hit_missing")?;
    let proof = import_normal_tools_cache_payload(&request, action)?;
    import_output(&proof, &inputs.cache_key, &inputs.stage_path)
}

fn payload_request(inputs: &Inputs) -> NormalToolsCachePayloadRequest<'_> {
    NormalToolsCachePayloadRequest {
        runner_temp: &inputs.runner_temp,
        home: &inputs.home,
        runtime_identity: &inputs.identity,
        cache_key: &inputs.cache_key,
        mise_data_dir: &inputs.mise_data_dir,
        cargo_home: &inputs.cargo_home,
        rustup_home: &inputs.rustup_home,
    }
}

fn export_output(proof: &CacheExportProof) -> Result<String, &'static str> {
    let stage_path = proof
        .save_stage_path
        .as_ref()
        .ok_or("normal_cache_export_stage_missing")?;
    let save_key = proof
        .save_key
        .as_ref()
        .ok_or("normal_cache_export_key_missing")?;
    let manifest_digest = proof
        .manifest_digest
        .as_ref()
        .ok_or("normal_cache_export_manifest_missing")?;
    output_body(&[
        ("export_result", "staged".to_owned()),
        ("save_key", save_key.clone()),
        ("stage_path", path_text(stage_path)?),
        ("state_digest", proof.state_digest.clone()),
        ("manifest_digest", manifest_digest.clone()),
        ("snapshot_admitted", "true".to_owned()),
    ])
}

fn import_output(
    proof: &CacheRestoreProof,
    cache_key: &str,
    stage_path: &Path,
) -> Result<String, &'static str> {
    let result = match proof.status {
        CacheRestoreStatus::Miss => "miss",
        CacheRestoreStatus::Hit => "hit",
        CacheRestoreStatus::Disabled => return Err("normal_cache_restore_disabled"),
    };
    let mut fields = vec![
        ("restore_result", result.to_owned()),
        ("restore_key", cache_key.to_owned()),
        ("stage_path", path_text(stage_path)?),
        (
            "state_digest",
            proof.state_digest.clone().unwrap_or_default(),
        ),
    ];
    if proof.status == CacheRestoreStatus::Hit {
        fields.push(("payload_admitted", "true".to_owned()));
        fields.push(("restore_admitted", "true".to_owned()));
    }
    output_body(&fields)
}

fn read_inputs(importing: bool) -> Result<Inputs, &'static str> {
    let runner_temp = required_path("RUNNER_TEMP")?;
    let home = required_path("HOME")?;
    if required_value(ENABLED_ENV)? != "true" {
        return Err("normal_cache_runtime_identity_disabled");
    }
    let identity = required_value(IDENTITY_ENV)?;
    let cache_key = required_value(KEY_ENV)?;
    let mise_data_dir = required_path(MISE_DATA_ENV)?;
    let cargo_home = required_path(CARGO_HOME_ENV)?;
    let rustup_home = required_path(RUSTUP_HOME_ENV)?;
    let stage_path = required_path(STAGE_PATH_ENV)?;
    let expected_stage = normal_tools_cache_stage_path(&runner_temp, &cache_key)?;
    if stage_path != expected_stage {
        return Err("normal_cache_stage_path_mismatch");
    }
    let action = if importing {
        Some(read_action()?)
    } else {
        None
    };
    Ok(Inputs {
        runner_temp,
        home,
        identity,
        cache_key,
        mise_data_dir,
        cargo_home,
        rustup_home,
        stage_path,
        action,
    })
}

fn read_action() -> Result<CacheActionObservation, &'static str> {
    let hit = match required_value(HIT_ENV)?.as_str() {
        "true" => true,
        "false" => false,
        _ => return Err("normal_cache_action_hit_invalid"),
    };
    let matched_key = optional_value(MATCHED_KEY_ENV);
    Ok(CacheActionObservation { hit, matched_key })
}

fn required_path(name: &str) -> Result<PathBuf, &'static str> {
    let path = PathBuf::from(required_value(name)?);
    if !path.is_absolute()
        || path.to_str().is_none_or(|text| {
            text.len() > 4_096
                || text
                    .split('/')
                    .skip(1)
                    .any(|part| part.is_empty() || part == "." || part == "..")
        })
    {
        return Err("normal_cache_path_invalid");
    }
    Ok(path)
}

fn required_value(name: &str) -> Result<String, &'static str> {
    let value = env::var(name)
        .ok()
        .filter(|value| !value.is_empty())
        .ok_or("normal_cache_input_missing")?;
    if value.len() > 4_096 || value.chars().any(|ch| matches!(ch, '\n' | '\r')) {
        return Err("normal_cache_input_invalid");
    }
    Ok(value)
}

fn optional_value(name: &str) -> Option<String> {
    env::var(name).ok().filter(|value| !value.is_empty())
}

fn path_text(path: &Path) -> Result<String, &'static str> {
    let text = path.to_str().ok_or("normal_cache_output_invalid")?;
    if text.len() > 4_096 || text.chars().any(|ch| matches!(ch, '\n' | '\r')) {
        return Err("normal_cache_output_invalid");
    }
    Ok(text.to_owned())
}

fn output_body(fields: &[(&str, String)]) -> Result<String, &'static str> {
    let mut body = String::new();
    for (name, value) in fields {
        if value.chars().any(|ch| matches!(ch, '\n' | '\r')) {
            return Err("normal_cache_output_invalid");
        }
        body.push_str(name);
        body.push('=');
        body.push_str(value);
        body.push('\n');
    }
    Ok(body)
}

fn append_output(body: &str, operation: &str) -> ExitCode {
    let Some(path) = env::var_os(super::super::GITHUB_OUTPUT_ENV).filter(|value| !value.is_empty())
    else {
        return fail(operation, "missing_github_output");
    };
    match super::super::append_owned_output(&path, body.as_bytes()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(_) => fail(operation, "append_github_output_failed"),
    }
}

fn fail(operation: &str, problem: &str) -> ExitCode {
    eprintln!("velnor-actions: {operation} failed: {problem}");
    ExitCode::from(1)
}
