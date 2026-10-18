//! Normal `OpenTofu` provider-cache runtime binding.

use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use velnor_actions_contract::{canonical_json_str, strict_json::parse_strict_json_with_limit};

const SCHEMA: &str = "velnor.tofu-cache-runtime/v1";
const IDENTITY_SCHEMA: &str = "velnor.tofu-cache-runtime-identity/v1";
const KEY_PREFIX: &str = "velnor-v1-tofu-providers-payload-v3";
const MAX_OBSERVATION_BYTES: usize = 65_536;
const MAX_LOCKFILE_BYTES: u64 = 4 * 1024 * 1024;
const MAX_BINARY_BYTES: u64 = 512 * 1024 * 1024;
const MAX_OPERATION_BYTES: u64 = 4 * 1024 * 1024 * 1024;

#[path = "normal_tofu_export.rs"]
mod export;
pub use export::{NormalTofuProvidersCacheExport, export_normal_tofu_providers_cache_payload};
#[path = "normal_tofu_admission.rs"]
mod admission;
pub use admission::{
    NormalTofuProvidersCacheBackendEvidence, NormalTofuProvidersCacheRestorePreflight,
    observe_normal_tofu_providers_cache_backend, preflight_normal_tofu_providers_cache_restore,
};

/// Inputs for one normal `OpenTofu` provider-cache payload operation.
#[derive(Debug, Clone, Copy)]
pub struct NormalTofuProvidersCachePayloadRequest<'a> {
    /// Runner temporary root from the current job.
    pub runner_temp: &'a Path,
    /// Current workspace root used to find the selected lockfile.
    pub workspace: &'a Path,
    /// Expected native target from the generated task.
    pub expected_target: &'a str,
    /// Exact `OpenTofu` pin from the generated task.
    pub expected_tofu_version: &'a str,
    /// Exact normalized root from the generated task.
    pub expected_root: &'a str,
    /// Concrete cache key emitted by the trusted runtime collector.
    pub cache_key: &'a str,
    /// Cache path emitted by the trusted runtime collector.
    pub cache_path: &'a Path,
    /// Current `TF_PLUGIN_CACHE_DIR` value.
    pub configured_plugin_cache_dir: &'a Path,
    /// Canonical output from the trusted post-setup runtime collector.
    pub runtime_observation: &'a str,
    /// Receipt claim from an earlier restore of the same one-file stage.
    pub previous_stage_claim: Option<&'a str>,
    /// Maximum bytes this operation may write to the one-file stage.
    pub operation_limit_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ValidatedTofuRuntime {
    pub(super) target: String,
    pub(super) tofu_version: String,
    pub(super) root: String,
    pub(super) lockfile_sha256: String,
    pub(super) runtime_identity_digest: String,
    pub(super) cache_path: PathBuf,
}

/// Validate the measured `ToFu` runtime and its exact cache identity.
///
/// # Errors
///
/// Gives a static error code when an observed value, file, key, or path does
/// not agree with the requested task.
pub fn validate_normal_tofu_providers_cache_request(
    request: &NormalTofuProvidersCachePayloadRequest<'_>,
) -> Result<(), &'static str> {
    validate_request(request).map(|_| ())
}

pub(super) fn validate_request(
    request: &NormalTofuProvidersCachePayloadRequest<'_>,
) -> Result<ValidatedTofuRuntime, &'static str> {
    validate_task_inputs(request)?;
    let runtime = parse_runtime(request.runtime_observation)?;
    if runtime.target != request.expected_target
        || runtime.tofu_version != request.expected_tofu_version
        || runtime.root != request.expected_root
    {
        return Err("normal_tofu_runtime_mismatch");
    }
    let derived_path = cache_path(request.runner_temp, request.expected_root)?;
    if request.cache_path != derived_path
        || request.configured_plugin_cache_dir != derived_path
        || runtime.plugin_cache_dir != path_text(&derived_path)?
    {
        return Err("normal_tofu_cache_path_mismatch");
    }
    let lockfile = lockfile_path(request.workspace, request.expected_root)?;
    let lockfile_sha256 = sha256_file(&lockfile, MAX_LOCKFILE_BYTES, false)?;
    if runtime.lockfile_sha256 != lockfile_sha256 {
        return Err("normal_tofu_lockfile_digest_mismatch");
    }
    let binary_path = PathBuf::from(&runtime.tofu_binary_path);
    clean_absolute(&binary_path)?;
    let binary_sha256 = sha256_file(&binary_path, MAX_BINARY_BYTES, true)?;
    if runtime.tofu_binary_sha256 != binary_sha256 {
        return Err("normal_tofu_binary_digest_mismatch");
    }
    let key = cache_key(
        request.expected_target,
        request.expected_tofu_version,
        request.expected_root,
        &lockfile_sha256,
    )?;
    if request.cache_key != key {
        return Err("normal_tofu_cache_key_mismatch");
    }
    let identity = canonical_json_str(&json!({
        "lockfile_sha256": lockfile_sha256,
        "root": request.expected_root,
        "schema": IDENTITY_SCHEMA,
        "target": request.expected_target,
        "tofu_binary_sha256": binary_sha256,
        "tofu_version": request.expected_tofu_version,
    }))
    .map_err(|_| "normal_tofu_runtime_invalid")?;
    let runtime_identity_digest = format!("b3-{}", blake3::hash(identity.as_bytes()).to_hex());
    Ok(ValidatedTofuRuntime {
        target: runtime.target,
        tofu_version: runtime.tofu_version,
        root: runtime.root,
        lockfile_sha256,
        runtime_identity_digest,
        cache_path: derived_path,
    })
}

pub(in crate::qualification_cache_payload) fn cache_key(
    target: &str,
    tofu_version: &str,
    root: &str,
    lockfile_sha256: &str,
) -> Result<String, &'static str> {
    validate_digest(lockfile_sha256)?;
    let locator =
        velnor_actions_tofu::tofu_root_locator(root).map_err(|_| "normal_tofu_root_invalid")?;
    let key = format!("{KEY_PREFIX}-{target}-{tofu_version}-{locator}-{lockfile_sha256}");
    if key.len() > velnor_actions_contract::cachekey::MAX_CACHE_KEY_BYTES
        || !key.bytes().all(|byte| byte.is_ascii_graphic())
    {
        return Err("normal_tofu_cache_key_invalid");
    }
    Ok(key)
}

/// Check the closed shape of a `ToFu` provider key before deriving its stage.
///
/// Full identity validation still recomputes this key from the measured lock
/// file and task inputs.
pub(crate) fn validate_cache_key_shape(key: &str) -> Result<(), &'static str> {
    let suffix = key
        .strip_prefix(&format!("{KEY_PREFIX}-"))
        .ok_or("normal_tofu_cache_key_invalid")?;
    let target = velnor_actions_contract::SUPPORTED_TARGETS
        .iter()
        .find(|target| suffix.starts_with(&format!("{target}-")))
        .ok_or("normal_tofu_cache_key_invalid")?;
    let version_and_digests = &suffix[target.len() + 1..];
    let (version, digests) = version_and_digests
        .split_once("-b3-")
        .ok_or("normal_tofu_cache_key_invalid")?;
    velnor_actions_mise::validate_exact_version("opentofu", version)
        .map_err(|_| "normal_tofu_cache_key_invalid")?;
    let (root_digest, lockfile_digest) = digests
        .split_once('-')
        .ok_or("normal_tofu_cache_key_invalid")?;
    if root_digest.len() != 64
        || !root_digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err("normal_tofu_cache_key_invalid");
    }
    validate_digest(lockfile_digest)?;
    if key.len() > velnor_actions_contract::cachekey::MAX_CACHE_KEY_BYTES
        || !key.bytes().all(|byte| byte.is_ascii_graphic())
    {
        return Err("normal_tofu_cache_key_invalid");
    }
    Ok(())
}

fn validate_task_inputs(
    request: &NormalTofuProvidersCachePayloadRequest<'_>,
) -> Result<(), &'static str> {
    clean_absolute(request.runner_temp)?;
    clean_absolute(request.workspace)?;
    if !velnor_actions_contract::is_supported_target(request.expected_target) {
        return Err("normal_tofu_target_invalid");
    }
    velnor_actions_mise::validate_exact_version("opentofu", request.expected_tofu_version)
        .map_err(|_| "normal_tofu_version_invalid")?;
    velnor_actions_tofu::validate_normalized_root(request.expected_root)
        .map_err(|_| "normal_tofu_root_invalid")?;
    if !(34 + 1_024..=MAX_OPERATION_BYTES).contains(&request.operation_limit_bytes) {
        return Err("normal_tofu_operation_limit_invalid");
    }
    Ok(())
}

fn parse_runtime(text: &str) -> Result<RuntimeObservation, &'static str> {
    if text.is_empty()
        || text.len() > MAX_OBSERVATION_BYTES
        || text.chars().any(|ch| matches!(ch, '\n' | '\r'))
    {
        return Err("normal_tofu_runtime_invalid");
    }
    let value = parse_strict_json_with_limit(text, MAX_OBSERVATION_BYTES)
        .map_err(|_| "normal_tofu_runtime_invalid")?;
    if canonical_json_str(&value).map_err(|_| "normal_tofu_runtime_invalid")? != text {
        return Err("normal_tofu_runtime_noncanonical");
    }
    let object = value
        .as_object()
        .filter(|object| {
            has_exact_keys(
                object,
                &[
                    "lockfile_sha256",
                    "plugin_cache_dir",
                    "root",
                    "schema",
                    "target",
                    "tofu_binary_path",
                    "tofu_binary_sha256",
                    "tofu_version",
                ],
            )
        })
        .ok_or("normal_tofu_runtime_invalid")?;
    if string_field(object, "schema")? != SCHEMA {
        return Err("normal_tofu_runtime_schema_invalid");
    }
    let lockfile_sha256 = string_field(object, "lockfile_sha256")?.to_owned();
    let tofu_binary_sha256 = string_field(object, "tofu_binary_sha256")?.to_owned();
    validate_digest(&lockfile_sha256)?;
    validate_digest(&tofu_binary_sha256)?;
    Ok(RuntimeObservation {
        target: string_field(object, "target")?.to_owned(),
        tofu_version: string_field(object, "tofu_version")?.to_owned(),
        root: object
            .get("root")
            .and_then(Value::as_str)
            .ok_or("normal_tofu_runtime_invalid")?
            .to_owned(),
        plugin_cache_dir: string_field(object, "plugin_cache_dir")?.to_owned(),
        lockfile_sha256,
        tofu_binary_path: string_field(object, "tofu_binary_path")?.to_owned(),
        tofu_binary_sha256,
    })
}

#[derive(Debug)]
struct RuntimeObservation {
    target: String,
    tofu_version: String,
    root: String,
    plugin_cache_dir: String,
    lockfile_sha256: String,
    tofu_binary_path: String,
    tofu_binary_sha256: String,
}

fn cache_path(runner_temp: &Path, root: &str) -> Result<PathBuf, &'static str> {
    let temp = path_text(runner_temp)?;
    let base = format!("{temp}/velnor/cache/tofu");
    let value = velnor_actions_tofu::tofu_cache_dir_under(&base, root)
        .map_err(|_| "normal_tofu_cache_path_invalid")?;
    let path = PathBuf::from(value);
    validate_plain_absolute(&path)?;
    Ok(path)
}

fn lockfile_path(workspace: &Path, root: &str) -> Result<PathBuf, &'static str> {
    let relative_root = if root.is_empty() {
        PathBuf::new()
    } else {
        PathBuf::from(root)
    };
    let path = workspace.join(relative_root).join(".terraform.lock.hcl");
    validate_plain_absolute(&path)?;
    Ok(path)
}

fn sha256_file(path: &Path, max_bytes: u64, executable: bool) -> Result<String, &'static str> {
    let (mut file, before, source_guard) = super::transfer::open_copy_source(path)?;
    if executable && before.st_mode & 0o111 == 0 {
        return Err("normal_tofu_binary_not_executable");
    }
    let declared = u64::try_from(before.st_size).map_err(|_| "normal_tofu_file_limit")?;
    if declared > max_bytes {
        return Err("normal_tofu_file_limit");
    }
    let mut digest = Sha256::new();
    let mut bytes = 0_u64;
    let mut buffer = vec![0_u8; 32 * 1024].into_boxed_slice();
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|_| "normal_tofu_file_read_failed")?;
        if count == 0 {
            break;
        }
        bytes = bytes
            .checked_add(u64::try_from(count).map_err(|_| "normal_tofu_file_limit")?)
            .filter(|size| *size <= max_bytes)
            .ok_or("normal_tofu_file_limit")?;
        digest.update(&buffer[..count]);
    }
    let after = rustix::fs::fstat(&file).map_err(|_| "normal_tofu_file_read_failed")?;
    if bytes != declared || !super::transfer::same_version(&before, &after) {
        return Err("normal_tofu_file_changed");
    }
    source_guard.revalidate(&file)?;
    Ok(lower_hex(&digest.finalize()))
}

pub(super) fn validate_sha256(value: &str) -> Result<(), &'static str> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err("normal_tofu_digest_invalid");
    }
    Ok(())
}

fn validate_digest(value: &str) -> Result<(), &'static str> {
    validate_sha256(value)
}

fn lower_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut value = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        value.push(char::from(HEX[usize::from(byte >> 4)]));
        value.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    value
}

fn clean_absolute(path: &Path) -> Result<(), &'static str> {
    validate_plain_absolute(path)?;
    let resolved = std::fs::canonicalize(path).map_err(|_| "normal_tofu_path_invalid")?;
    if resolved != path {
        return Err("normal_tofu_path_not_canonical");
    }
    Ok(())
}

fn validate_plain_absolute(path: &Path) -> Result<(), &'static str> {
    let text = path.to_str().ok_or("normal_tofu_path_invalid")?;
    if text.len() > 4_096
        || !text.starts_with('/')
        || text == "/"
        || text.bytes().any(|byte| byte.is_ascii_control())
        || path.components().any(|component| {
            matches!(
                component,
                Component::CurDir | Component::ParentDir | Component::Prefix(_)
            )
        })
    {
        return Err("normal_tofu_path_invalid");
    }
    Ok(())
}

fn path_text(path: &Path) -> Result<&str, &'static str> {
    path.to_str().ok_or("normal_tofu_path_invalid")
}

fn has_exact_keys(object: &Map<String, Value>, keys: &[&str]) -> bool {
    object.len() == keys.len() && keys.iter().all(|key| object.contains_key(*key))
}

fn string_field<'a>(object: &'a Map<String, Value>, key: &str) -> Result<&'a str, &'static str> {
    object
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or("normal_tofu_runtime_invalid")
}

#[cfg(test)]
#[path = "tests/normal_tofu_runtime_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "tests/normal_tofu_root_path_tests.rs"]
mod root_path_tests;
