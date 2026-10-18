//! Check normal `Tofu` cache identity and storage capacity before SDK download.

use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

use velnor_actions_orchestrator::{
    NormalTofuProvidersCachePayloadRequest, NormalTofuProvidersCacheRestorePreflight,
    preflight_normal_tofu_providers_cache_restore,
};

pub(crate) const OP: &str = "preflight-normal-tofu-providers-cache-restore-v1";

const MAX_VALUE_BYTES: usize = 65_536;
const MAX_OPERATION_BYTES: u64 = 4 * 1024 * 1024 * 1024;
const MIN_OPERATION_BYTES: u64 = 1_058;

pub(crate) fn run() -> ExitCode {
    match run_preflight() {
        Ok(body) => append_output(&body),
        Err(problem) => fail(problem),
    }
}

fn run_preflight() -> Result<String, &'static str> {
    if !super::cache_runner_context_is_valid() {
        return Err("normal_tofu_preflight_runner_context_invalid");
    }
    let inputs = Inputs::read()?;
    let preflight = preflight_normal_tofu_providers_cache_restore(&inputs.request())?;
    render_outputs(&preflight)
}

fn render_outputs(
    preflight: &NormalTofuProvidersCacheRestorePreflight,
) -> Result<String, &'static str> {
    let Some(entry) = preflight.cache_entry() else {
        return output_body(&[("preflight_result", "absent".to_owned())]);
    };
    let compression = preflight
        .compression()
        .ok_or("normal_tofu_preflight_compression_missing")?;
    let max_download_bytes = preflight
        .max_download_bytes()
        .ok_or("normal_tofu_preflight_download_limit_missing")?;
    let max_raw_stage_bytes = preflight
        .max_raw_stage_bytes()
        .ok_or("normal_tofu_preflight_stage_limit_missing")?;
    output_body(&[
        ("preflight_result", "ready".to_owned()),
        ("cache_key", preflight.cache_key().to_owned()),
        ("cache_entry_id", entry.id.to_string()),
        ("cache_entry_ref", entry.git_ref.clone()),
        ("cache_entry_version", entry.version.clone()),
        ("cache_entry_size_bytes", entry.size_bytes.to_string()),
        ("cache_compression", compression.as_str().to_owned()),
        ("max_download_bytes", max_download_bytes.to_string()),
        ("max_raw_stage_bytes", max_raw_stage_bytes.to_string()),
    ])
}

#[derive(Debug)]
struct Inputs {
    runner_temp: PathBuf,
    workspace: PathBuf,
    target: String,
    tofu_version: String,
    root: String,
    cache_key: String,
    cache_path: PathBuf,
    configured_plugin_cache_dir: PathBuf,
    runtime_observation: String,
    operation_limit_bytes: u64,
}

impl Inputs {
    fn read() -> Result<Self, &'static str> {
        Ok(Self {
            runner_temp: required_path("RUNNER_TEMP")?,
            workspace: required_path("GITHUB_WORKSPACE")?,
            target: required_value("VELNOR_TOFU_CACHE_TARGET", 256)?,
            tofu_version: required_value("VELNOR_TOFU_CACHE_VERSION", 128)?,
            root: required_value("VELNOR_TOFU_CACHE_ROOT", 4_096)?,
            cache_key: required_value("VELNOR_TOFU_CACHE_KEY", 4_096)?,
            cache_path: required_path("VELNOR_TOFU_CACHE_PATH")?,
            configured_plugin_cache_dir: required_path("TF_PLUGIN_CACHE_DIR")?,
            runtime_observation: required_value(
                "VELNOR_TOFU_CACHE_RUNTIME_OBSERVATION",
                MAX_VALUE_BYTES,
            )?,
            operation_limit_bytes: operation_limit()?,
        })
    }

    fn request(&self) -> NormalTofuProvidersCachePayloadRequest<'_> {
        NormalTofuProvidersCachePayloadRequest {
            runner_temp: &self.runner_temp,
            workspace: &self.workspace,
            expected_target: &self.target,
            expected_tofu_version: &self.tofu_version,
            expected_root: &self.root,
            cache_key: &self.cache_key,
            cache_path: &self.cache_path,
            configured_plugin_cache_dir: &self.configured_plugin_cache_dir,
            runtime_observation: &self.runtime_observation,
            previous_stage_claim: None,
            operation_limit_bytes: self.operation_limit_bytes,
        }
    }
}

fn operation_limit() -> Result<u64, &'static str> {
    let text = required_value("VELNOR_CACHE_OPERATION_LIMIT_BYTES", 20)?;
    let value = text
        .parse::<u64>()
        .map_err(|_| "normal_tofu_preflight_operation_limit_invalid")?;
    if value.to_string() != text || !(MIN_OPERATION_BYTES..=MAX_OPERATION_BYTES).contains(&value) {
        return Err("normal_tofu_preflight_operation_limit_invalid");
    }
    Ok(value)
}

fn required_path(name: &str) -> Result<PathBuf, &'static str> {
    let path = PathBuf::from(required_value(name, 4_096)?);
    let text = path.to_str().ok_or("normal_tofu_preflight_path_invalid")?;
    if !path.is_absolute()
        || text
            .split('/')
            .skip(1)
            .any(|part| part.is_empty() || matches!(part, "." | ".."))
    {
        return Err("normal_tofu_preflight_path_invalid");
    }
    Ok(path)
}

fn required_value(name: &str, max_bytes: usize) -> Result<String, &'static str> {
    let value = env::var(name)
        .ok()
        .filter(|value| !value.is_empty())
        .ok_or("normal_tofu_preflight_input_missing")?;
    if value.len() > max_bytes || value.chars().any(|ch| matches!(ch, '\n' | '\r' | '\0')) {
        return Err("normal_tofu_preflight_input_invalid");
    }
    Ok(value)
}

fn output_body(fields: &[(&str, String)]) -> Result<String, &'static str> {
    let mut body = String::new();
    for (name, value) in fields {
        if value.chars().any(|ch| matches!(ch, '\n' | '\r' | '\0')) {
            return Err("normal_tofu_preflight_output_invalid");
        }
        body.push_str(name);
        body.push('=');
        body.push_str(value);
        body.push('\n');
    }
    Ok(body)
}

fn append_output(body: &str) -> ExitCode {
    let Some(path) = env::var_os(super::GITHUB_OUTPUT_ENV).filter(|value| !value.is_empty()) else {
        return fail("normal_tofu_preflight_output_missing");
    };
    match super::cache_bind::append_owned_output(&path, body.as_bytes()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(_) => fail("normal_tofu_preflight_output_write_failed"),
    }
}

fn fail(problem: &str) -> ExitCode {
    eprintln!("velnor-actions: normal ToFu cache preflight failed: {problem}");
    ExitCode::from(1)
}
