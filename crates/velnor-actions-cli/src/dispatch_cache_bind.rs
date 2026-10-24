//! Private operation for one plan-bound cache layer.

use std::env;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use velnor_actions_orchestrator::{
    QUALIFICATION_CACHE_BIND_OP, QualificationCacheBindingRequest,
    bind_qualification_cache_runtime, effective_uid,
};

#[path = "dispatch_cache_normal_stage.rs"]
pub(crate) mod normal_stage;
#[path = "dispatch_cache_transfer.rs"]
pub(crate) mod transfer;

/// Operation tag for one validated cache binding.
pub(crate) const OP: &str = QUALIFICATION_CACHE_BIND_OP;

pub(crate) const GITHUB_OUTPUT_ENV: &str = "GITHUB_OUTPUT";
const RUNNER_TEMP_ENV: &str = "RUNNER_TEMP";
const OWNER_ID_ENV: &str = "VELNOR_QUALIFICATION_CACHE_OWNER_ID";
const LAYER_ENV: &str = "VELNOR_QUALIFICATION_CACHE_LAYER";
const RUNTIME_IDENTITY_PATH_ENV: &str = "VELNOR_QUALIFICATION_RUNTIME_IDENTITY_FILE";
const ROOTS_FILE_ENV: &str = "VELNOR_QUALIFICATION_CACHE_ROOTS_FILE";
const RUNTIME_PHASE_ENV: &str = "VELNOR_QUALIFICATION_CACHE_RUNTIME_PHASE";

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
    owner_id: String,
    layer: String,
    runtime_identity_path: PathBuf,
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
            owner_id: &self.owner_id,
            layer: &self.layer,
            runtime_identity_path: &self.runtime_identity_path,
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
    if let Err(problem) = validate_optional_root_observation(&inputs) {
        return fail(problem);
    }
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

fn validate_optional_root_observation(inputs: &BindInputs) -> Result<(), &'static str> {
    let Ok(path) = env::var(ROOTS_FILE_ENV) else {
        return Ok(());
    };
    let request = inputs.request();
    let text = velnor_actions_orchestrator::read_qualification_cache_roots(
        &request,
        PathBuf::from(path).as_path(),
    )
    .map_err(|_| "qualification_cache_roots_untrusted")?;
    transfer::validate_root_observation(&request, &text)
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
        || stat.st_uid != effective_uid()
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
    let owner_id = required(&mut get, OWNER_ID_ENV)?;
    let layer = required(&mut get, LAYER_ENV)?;
    let runtime_phase = required(&mut get, RUNTIME_PHASE_ENV)?;
    let runtime_identity_filename = match runtime_phase.as_str() {
        "before_import" => {
            velnor_actions_orchestrator::qualification_cache_runtime_identity_filename(
                &owner_id, &layer,
            )
        }
        "after_import" => {
            velnor_actions_orchestrator::qualification_cache_runtime_after_import_filename(
                &owner_id, &layer,
            )
        }
        _ => return Err("bad_runtime_phase".to_owned()),
    }
    .map_err(|_| "bad_runtime_identity_name".to_owned())?;
    let expected_runtime_identity_path = runner_temp
        .join("velnor")
        .join(format!("r{run_id}-a{run_attempt}"))
        .join(runtime_identity_filename);
    let runtime_identity_path = match optional(&mut get, RUNTIME_IDENTITY_PATH_ENV) {
        Some(path) => {
            let path = required_path_value(path)?;
            if path != expected_runtime_identity_path {
                return Err("runtime_identity_path_mismatch".to_owned());
            }
            path
        }
        None => expected_runtime_identity_path,
    };
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
        owner_id,
        layer,
        runtime_identity_path,
    })
}

#[cfg(test)]
#[path = "dispatch_cache_bind_tests.rs"]
mod tests;

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
    required_path_value(text)
}

fn required_path_value(text: String) -> Result<PathBuf, String> {
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
    let fields = output_fields(outputs, runner_temp)?;
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

fn output_fields(
    outputs: &velnor_actions_orchestrator::QualificationCacheBindingOutputs,
    runner_temp: &std::path::Path,
) -> Result<Vec<(&'static str, String)>, &'static str> {
    let cache = outputs.expected_cache.as_ref();
    let restore_stage_path = stage_path(outputs, runner_temp, outputs.restore_key.as_deref())?;
    let save_stage_path = stage_path(outputs, runner_temp, outputs.save_key.as_deref())?;
    let stage_path = if restore_stage_path.is_empty() {
        save_stage_path.clone()
    } else {
        restore_stage_path.clone()
    };
    Ok(vec![
        ("active", bool_text(outputs.active).to_owned()),
        ("campaign", outputs.campaign.clone()),
        ("phase", outputs.phase.clone()),
        ("plan_id", outputs.plan_id.clone()),
        ("run_key", outputs.run_key.clone()),
        ("owner_id", outputs.owner_id.clone()),
        ("matrix_key", outputs.matrix_key.clone()),
        ("layer", outputs.layer.clone()),
        (
            "restore_key",
            outputs.restore_key.clone().unwrap_or_default(),
        ),
        ("restore_stage_path", restore_stage_path),
        ("stage_path", stage_path),
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
    ])
}

fn stage_path(
    outputs: &velnor_actions_orchestrator::QualificationCacheBindingOutputs,
    runner_temp: &std::path::Path,
    key: Option<&str>,
) -> Result<String, &'static str> {
    key.map(|key| {
        velnor_actions_orchestrator::qualification_cache_stage_path(
            runner_temp,
            &outputs.campaign,
            &outputs.layer,
            key,
        )
        .map(|path| path.display().to_string())
    })
    .transpose()
    .map(Option::unwrap_or_default)
}

fn bool_text(value: bool) -> &'static str {
    if value { "true" } else { "false" }
}

fn fail(problem: &str) -> ExitCode {
    eprintln!("velnor-actions: cache binding failed: {problem}");
    ExitCode::from(1)
}
