//! Private operation for one plan-bound cache layer.

use std::env;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use velnor_actions_orchestrator::{
    QUALIFICATION_CACHE_BIND_OP, QualificationCacheBindingRequest, bind_qualification_cache_runtime,
};

/// Operation tag for one validated cache binding.
pub(crate) const OP: &str = QUALIFICATION_CACHE_BIND_OP;

const MAX_RUNTIME_IDENTITY_BYTES: usize = 65_536;
pub(crate) const GITHUB_OUTPUT_ENV: &str = "GITHUB_OUTPUT";
const RUNNER_TEMP_ENV: &str = "RUNNER_TEMP";
const MATRIX_KEY_ENV: &str = "VELNOR_QUALIFICATION_CACHE_MATRIX_KEY";
const LAYER_ENV: &str = "VELNOR_QUALIFICATION_CACHE_LAYER";
const RUNTIME_IDENTITY_ENV: &str = "VELNOR_QUALIFICATION_RUNTIME_IDENTITY";
const QUALIFICATION_CACHE_STAGE_ROOT: &str = "velnor/qualification-cache-v2";

#[derive(Debug)]
pub(crate) struct BindInputs {
    runner_temp: PathBuf,
    run_id: u64,
    run_attempt: u32,
    event_name: String,
    repository: String,
    git_ref: String,
    ref_protected: bool,
    workflow_ref: String,
    workflow_sha: String,
    source_sha: String,
    runner_os: String,
    runner_arch: String,
    image_os: Option<String>,
    image_version: Option<String>,
    matrix_key: String,
    layer: String,
    runtime_identity_json: String,
}

impl BindInputs {
    pub(crate) fn request(&self) -> QualificationCacheBindingRequest<'_> {
        QualificationCacheBindingRequest {
            runner_temp: &self.runner_temp,
            run_id: self.run_id,
            run_attempt: self.run_attempt,
            event_name: &self.event_name,
            repository: &self.repository,
            git_ref: &self.git_ref,
            ref_protected: self.ref_protected,
            workflow_ref: &self.workflow_ref,
            workflow_sha: &self.workflow_sha,
            source_sha: &self.source_sha,
            runner_os: &self.runner_os,
            runner_arch: &self.runner_arch,
            image_os: self.image_os.as_deref(),
            image_version: self.image_version.as_deref(),
            matrix_key: &self.matrix_key,
            layer: &self.layer,
            runtime_identity_json: &self.runtime_identity_json,
        }
    }
}

/// Bind one typed directive and append its exact key outputs.
pub(crate) fn run() -> ExitCode {
    let inputs = match read_inputs(|name| env::var(name)) {
        Ok(inputs) => inputs,
        Err(problem) => return fail(&problem),
    };
    let outputs = match bind_qualification_cache_runtime(&inputs.request()) {
        Ok(outputs) => outputs,
        Err(error) => return fail(&error.to_string()),
    };
    let Some(path) = env::var_os(GITHUB_OUTPUT_ENV).filter(|value| !value.is_empty()) else {
        return fail("missing_github_output");
    };
    let body = match output_body(&outputs, &inputs.runner_temp) {
        Ok(body) => body,
        Err(problem) => return fail(problem),
    };
    match append_owned_output(&path, body.as_bytes()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => fail(&format!("append_github_output:{error}")),
    }
}

pub(crate) fn append_owned_output(path: &std::ffi::OsStr, body: &[u8]) -> std::io::Result<()> {
    let path = PathBuf::from(path);
    let text = path.to_str().ok_or_else(invalid_output_path)?;
    if text.len() > 4_096
        || !path.is_absolute()
        || text
            .split('/')
            .skip(1)
            .any(|part| part.is_empty() || part == "." || part == "..")
        || text
            .bytes()
            .any(|byte| byte == 0 || byte == b'\n' || byte == b'\r')
    {
        return Err(invalid_output_path());
    }
    let fd = rustix::fs::open(
        &path,
        rustix::fs::OFlags::WRONLY
            | rustix::fs::OFlags::APPEND
            | rustix::fs::OFlags::NOFOLLOW
            | rustix::fs::OFlags::NONBLOCK
            | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
    )
    .map_err(std::io::Error::from)?;
    let stat = rustix::fs::fstat(&fd).map_err(std::io::Error::from)?;
    if rustix::fs::FileType::from_raw_mode(stat.st_mode) != rustix::fs::FileType::RegularFile
        || stat.st_uid != rustix::process::geteuid().as_raw()
    {
        return Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied));
    }
    let mut file = fs::File::from(fd);
    file.write_all(body)
}

fn invalid_output_path() -> std::io::Error {
    std::io::Error::from(std::io::ErrorKind::InvalidInput)
}

pub(crate) fn read_inputs<E>(mut get: E) -> Result<BindInputs, String>
where
    E: FnMut(&str) -> Result<String, env::VarError>,
{
    let runner_temp = required_path(&mut get, RUNNER_TEMP_ENV)?;
    let run_id_text = required(&mut get, "GITHUB_RUN_ID")?;
    let run_id = parse_decimal(&run_id_text, "run_id")?;
    let attempt_text = required(&mut get, "GITHUB_RUN_ATTEMPT")?;
    let run_attempt = u32::try_from(parse_decimal(&attempt_text, "run_attempt")?)
        .map_err(|_| "bad_run_attempt".to_owned())?;
    let ref_protected = match required(&mut get, "GITHUB_REF_PROTECTED")?.as_str() {
        "true" => true,
        "false" => false,
        _ => return Err("bad_ref_protected".to_owned()),
    };
    let runtime_identity_json = required(&mut get, RUNTIME_IDENTITY_ENV)?;
    if runtime_identity_json.len() > MAX_RUNTIME_IDENTITY_BYTES
        || runtime_identity_json
            .chars()
            .any(|value| matches!(value, '\n' | '\r'))
    {
        return Err("cache_binding_input_json_invalid".to_owned());
    }
    Ok(BindInputs {
        runner_temp,
        run_id,
        run_attempt,
        event_name: required(&mut get, "GITHUB_EVENT_NAME")?,
        repository: required(&mut get, "GITHUB_REPOSITORY")?,
        git_ref: required(&mut get, "GITHUB_REF")?,
        ref_protected,
        workflow_ref: required(&mut get, "GITHUB_WORKFLOW_REF")?,
        workflow_sha: required(&mut get, "GITHUB_WORKFLOW_SHA")?,
        source_sha: required(&mut get, "GITHUB_SHA")?,
        runner_os: required(&mut get, "RUNNER_OS")?,
        runner_arch: required(&mut get, "RUNNER_ARCH")?,
        image_os: optional(&mut get, "ImageOS"),
        image_version: optional(&mut get, "ImageVersion"),
        matrix_key: required(&mut get, MATRIX_KEY_ENV)?,
        layer: required(&mut get, LAYER_ENV)?,
        runtime_identity_json,
    })
}

fn optional<E>(get: &mut E, name: &str) -> Option<String>
where
    E: FnMut(&str) -> Result<String, env::VarError>,
{
    get(name).ok().filter(|value| !value.is_empty())
}

fn required<E>(get: &mut E, name: &str) -> Result<String, String>
where
    E: FnMut(&str) -> Result<String, env::VarError>,
{
    get(name)
        .ok()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("missing_{}", name.to_ascii_lowercase()))
}

fn required_path<E>(get: &mut E, name: &str) -> Result<PathBuf, String>
where
    E: FnMut(&str) -> Result<String, env::VarError>,
{
    let text = required(get, name)?;
    if text.len() > 4_096
        || !text.starts_with('/')
        || text
            .bytes()
            .any(|byte| byte == 0 || byte == b'\n' || byte == b'\r')
        || text
            .split('/')
            .skip(1)
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err("runner_temp_path_invalid".to_owned());
    }
    Ok(PathBuf::from(text))
}

fn parse_decimal(text: &str, field: &str) -> Result<u64, String> {
    let value = text.parse::<u64>().map_err(|_| format!("bad_{field}"))?;
    if value == 0 || value.to_string() != text {
        return Err(format!("bad_{field}"));
    }
    Ok(value)
}

fn output_body(
    outputs: &velnor_actions_orchestrator::QualificationCacheBindingOutputs,
    runner_temp: &std::path::Path,
) -> Result<String, &'static str> {
    let cache = outputs.expected_cache.as_ref();
    let restore_stage_path = outputs
        .restore_key
        .as_deref()
        .map(|key| stage_path(runner_temp, &outputs.campaign, &outputs.layer, key))
        .transpose()?
        .unwrap_or_default();
    let save_stage_path = outputs
        .save_key
        .as_deref()
        .map(|key| stage_path(runner_temp, &outputs.campaign, &outputs.layer, key))
        .transpose()?
        .unwrap_or_default();
    let fields = [
        ("active", bool_text(outputs.active).to_owned()),
        ("campaign", outputs.campaign.clone()),
        ("phase", outputs.phase.clone()),
        ("plan_id", outputs.plan_id.clone()),
        ("run_key", outputs.run_key.clone()),
        ("matrix_key", outputs.matrix_key.clone()),
        ("layer", outputs.layer.clone()),
        (
            "restore_key",
            outputs.restore_key.clone().unwrap_or_default(),
        ),
        ("restore_stage_path", restore_stage_path),
        (
            "expected_cache_id",
            cache.map_or_else(String::new, |entry| entry.id.to_string()),
        ),
        (
            "expected_cache_key",
            cache.map_or_else(String::new, |entry| entry.key.clone()),
        ),
        (
            "expected_cache_ref",
            cache.map_or_else(String::new, |entry| entry.git_ref.clone()),
        ),
        (
            "expected_cache_version",
            cache.map_or_else(String::new, |entry| entry.version.clone()),
        ),
        (
            "expected_cache_size",
            cache.map_or_else(String::new, |entry| entry.size_bytes.to_string()),
        ),
        ("save_key", outputs.save_key.clone().unwrap_or_default()),
        ("save_stage_path", save_stage_path),
        (
            "save_if_state_changes",
            bool_text(outputs.save_if_state_changes).to_owned(),
        ),
        (
            "expected_prior_state_digest",
            outputs
                .expected_prior_state_digest
                .clone()
                .unwrap_or_default(),
        ),
        (
            "runtime_identity_digest",
            outputs.runtime_identity_digest.clone().unwrap_or_default(),
        ),
    ];
    let mut body = String::new();
    for (name, value) in fields {
        if value.chars().any(|ch| matches!(ch, '\n' | '\r')) {
            return Err("cache_binding_output_invalid");
        }
        body.push_str(name);
        body.push('=');
        body.push_str(&value);
        body.push('\n');
    }
    Ok(body)
}

/// Stable absolute staging path for an exact, typed campaign/layer/cache key.
/// The path deliberately excludes the workflow run/attempt so a cold save and
/// warm restore of the same key use the same action-cache archive version.
fn stage_path(
    runner_temp: &std::path::Path,
    campaign: &str,
    layer: &str,
    key: &str,
) -> Result<String, &'static str> {
    let runner_temp = runner_temp
        .to_str()
        .ok_or("cache_binding_stage_path_invalid")?;
    if runner_temp.len() > 4_096
        || !runner_temp.starts_with('/')
        || runner_temp == "/"
        || runner_temp
            .bytes()
            .any(|byte| matches!(byte, 0 | b'\n' | b'\r'))
        || runner_temp
            .split('/')
            .skip(1)
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err("cache_binding_stage_path_invalid");
    }
    if [campaign, layer, key]
        .iter()
        .any(|value| value.chars().any(|ch| matches!(ch, '\n' | '\r')))
    {
        return Err("cache_binding_output_invalid");
    }
    let valid_campaign = !campaign.is_empty()
        && campaign.len() <= 64
        && campaign
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-');
    let valid_layer = matches!(
        layer,
        "mbx_objects" | "cargo_sources" | "mise_tools" | "tofu_providers" | "task_result"
    );
    let valid_key = !key.is_empty()
        && key.len() <= velnor_actions_contract::cachekey::MAX_CACHE_KEY_BYTES
        && key.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"-_".contains(&byte)
        })
        && key.starts_with(&format!("velnor-qcache-v1-{campaign}-{layer}-"));
    if !valid_campaign || !valid_layer || !valid_key {
        return Err("cache_binding_stage_path_invalid");
    }
    let key_digest = blake3::hash(key.as_bytes()).to_hex();
    let path = format!(
        "{runner_temp}/{QUALIFICATION_CACHE_STAGE_ROOT}/{campaign}/{layer}/{key_digest}/payload"
    );
    if path.len() > 4_096 {
        return Err("cache_binding_stage_path_invalid");
    }
    Ok(path)
}

fn bool_text(value: bool) -> &'static str {
    if value { "true" } else { "false" }
}

fn fail(problem: &str) -> ExitCode {
    eprintln!("velnor-actions: cache binding failed: {problem}");
    ExitCode::from(1)
}

#[cfg(test)]
#[path = "dispatch_cache_bind_tests.rs"]
mod tests;
