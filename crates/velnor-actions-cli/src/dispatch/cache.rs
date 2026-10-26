// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use velnor_actions_orchestrator::{
    AdmissionDecision, inventory_digest, parse_admission_bytes, parse_inventory_bytes,
};

use super::fail_internal;
use super::operation::InternalOp;

pub(super) const CACHE_PUBLISH_OP: &str = "cache-publish-v1";
pub(super) const CACHE_RECOVER_OP: &str = "cache-recover-v1";
pub(super) const CACHE_CLEANUP_OP: &str = "cache-cleanup-v1";

const CACHE_SCHEMA: u32 = 1;
const REQUEST_DIR: &str = "velnor";
const RUNNER_TEMP_ENV: &str = "RUNNER_TEMP";
const REQUEST_FILE_ENV: &str = "VELNOR_REQUEST_FILE";
const PUBLISH_REQUEST_FILE: &str = "cache-publish.json";
const RECOVER_REQUEST_FILE: &str = "cache-recover.json";
const CLEANUP_REQUEST_FILE: &str = "cache-cleanup.json";
const MAX_CACHE_REQUEST_BYTES: u64 = 64 * 1024;
const MAX_CLEANUP_CANDIDATES: usize = 8;

const COMMAND_KEYS: [&str; 8] = [
    "command",
    "argv",
    "shell",
    "exec",
    "script",
    "run",
    "entrypoint",
    "container",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CacheOp {
    Publish,
    Recover,
    Cleanup,
}

impl CacheOp {
    fn from_internal(op: InternalOp) -> Option<Self> {
        match op {
            InternalOp::CachePublish => Some(Self::Publish),
            InternalOp::CacheRecover => Some(Self::Recover),
            InternalOp::CacheCleanup => Some(Self::Cleanup),
            _ => None,
        }
    }

    fn operation_name(self) -> &'static str {
        match self {
            Self::Publish => "publish",
            Self::Recover => "recover",
            Self::Cleanup => "cleanup",
        }
    }

    fn request_file(self) -> &'static str {
        match self {
            Self::Publish => PUBLISH_REQUEST_FILE,
            Self::Recover => RECOVER_REQUEST_FILE,
            Self::Cleanup => CLEANUP_REQUEST_FILE,
        }
    }
}

struct ValidatedCacheRequest {
    admission_bytes: Vec<u8>,
    inventory_bytes: Vec<u8>,
    candidate_ids: Vec<u64>,
}

pub(super) fn gate_request(op: InternalOp, path: &Path) -> Option<PathBuf> {
    let cache_op = CacheOp::from_internal(op)?;
    let runner_temp = env::var_os(RUNNER_TEMP_ENV)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)?;
    if !runner_temp.is_absolute() {
        return None;
    }
    let root = runner_temp.join(REQUEST_DIR);
    if path != root.join(cache_op.request_file()) {
        return None;
    }
    if env::var_os(REQUEST_FILE_ENV).as_deref() != Some(path.as_os_str()) {
        return None;
    }
    let root_meta = fs::symlink_metadata(&root).ok()?;
    if root_meta.file_type().is_symlink() || !root_meta.is_dir() {
        return None;
    }
    let request_meta = fs::symlink_metadata(path).ok()?;
    if request_meta.file_type().is_symlink() || !request_meta.is_file() {
        return None;
    }
    if request_meta.len() == 0 || request_meta.len() > MAX_CACHE_REQUEST_BYTES {
        return None;
    }
    Some(path.to_path_buf())
}

pub(super) fn run(op: InternalOp, path: &Path) -> ExitCode {
    let Some(cache_op) = CacheOp::from_internal(op) else {
        return fail_internal("cache_operation_malformed");
    };
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) => return fail_internal(&format!("read cache request: {error}")),
    };
    let request = match validate_request(cache_op, &bytes) {
        Ok(request) => request,
        Err(problem) => return fail_internal(&problem),
    };
    match cache_op {
        CacheOp::Publish => run_publish(&request.admission_bytes),
        CacheOp::Recover => run_recover(&request.inventory_bytes),
        CacheOp::Cleanup => run_cleanup(&request.inventory_bytes, &request.candidate_ids),
    }
}

fn validate_request(op: CacheOp, bytes: &[u8]) -> Result<ValidatedCacheRequest, String> {
    if bytes.is_empty() || u64::try_from(bytes.len()).is_ok_and(|len| len > MAX_CACHE_REQUEST_BYTES)
    {
        return Err("cache_request_malformed".to_owned());
    }
    let value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|_| "cache_request_malformed".to_owned())?;
    let body = value
        .as_object()
        .ok_or_else(|| "cache_request_malformed".to_owned())?;
    check_schema(body)?;
    check_operation(op, body)?;
    check_fields(op, body)?;
    Ok(ValidatedCacheRequest {
        admission_bytes: required_object(op, body, "admission")?,
        inventory_bytes: required_object(op, body, "inventory")?,
        candidate_ids: optional_candidate_ids(op, body)?,
    })
}

fn check_schema(body: &serde_json::Map<String, serde_json::Value>) -> Result<(), String> {
    let schema = body
        .get("schema")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| "cache_request_schema_malformed".to_owned())?;
    if schema != u64::from(CACHE_SCHEMA) {
        return Err("cache_request_schema_unsupported".to_owned());
    }
    Ok(())
}

fn check_operation(
    op: CacheOp,
    body: &serde_json::Map<String, serde_json::Value>,
) -> Result<(), String> {
    let operation = body
        .get("operation")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "cache_operation_malformed".to_owned())?;
    if operation != op.operation_name() {
        return Err("cache_operation_malformed".to_owned());
    }
    Ok(())
}

fn check_fields(
    op: CacheOp,
    body: &serde_json::Map<String, serde_json::Value>,
) -> Result<(), String> {
    for key in body.keys() {
        if COMMAND_KEYS.contains(&key.as_str()) {
            return Err("cache_request_rejected_command".to_owned());
        }
        if !is_allowed_field(op, key) {
            return Err("cache_request_field_rejected".to_owned());
        }
    }
    Ok(())
}

fn is_allowed_field(op: CacheOp, key: &str) -> bool {
    match op {
        CacheOp::Publish => matches!(key, "schema" | "operation" | "admission"),
        CacheOp::Recover => matches!(key, "schema" | "operation" | "inventory"),
        CacheOp::Cleanup => matches!(key, "schema" | "operation" | "inventory" | "candidate_ids"),
    }
}

fn required_object(
    op: CacheOp,
    body: &serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Result<Vec<u8>, String> {
    if !is_allowed_field(op, key) {
        return Ok(Vec::new());
    }
    let object = body
        .get(key)
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| format!("cache_{key}_malformed"))?;
    serde_json::to_vec(object).map_err(|_| format!("cache_{key}_malformed"))
}

fn optional_candidate_ids(
    op: CacheOp,
    body: &serde_json::Map<String, serde_json::Value>,
) -> Result<Vec<u64>, String> {
    if !matches!(op, CacheOp::Cleanup) {
        return Ok(Vec::new());
    }
    let ids = body
        .get("candidate_ids")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| "cache_candidate_ids_malformed".to_owned())?;
    if ids.is_empty() || ids.len() > MAX_CLEANUP_CANDIDATES {
        return Err("cache_candidate_ids_malformed".to_owned());
    }
    let mut candidates = Vec::with_capacity(ids.len());
    for id in ids {
        let id = id
            .as_u64()
            .filter(|value| *value != 0)
            .ok_or_else(|| "cache_candidate_id_malformed".to_owned())?;
        if candidates.contains(&id) {
            return Err("cache_candidate_id_duplicate".to_owned());
        }
        candidates.push(id);
    }
    Ok(candidates)
}

fn run_publish(admission_bytes: &[u8]) -> ExitCode {
    let admission = match parse_admission_bytes(admission_bytes) {
        Ok(admission) => admission,
        Err(error) => return fail_internal(&single_line(&error.to_string())),
    };
    match admission.decision {
        AdmissionDecision::Admitted => ExitCode::SUCCESS,
        AdmissionDecision::Blocked => fail_internal(&format!(
            "cache publish blocked: {}",
            admission.blocked_reasons.join(",")
        )),
    }
}

fn run_recover(inventory_bytes: &[u8]) -> ExitCode {
    let inventory = match parse_inventory_bytes(inventory_bytes) {
        Ok(inventory) => inventory,
        Err(error) => return fail_internal(&single_line(&error.to_string())),
    };
    match inventory_digest(&inventory) {
        Ok(_) => ExitCode::SUCCESS,
        Err(error) => fail_internal(&single_line(&error.to_string())),
    }
}

fn run_cleanup(inventory_bytes: &[u8], candidate_ids: &[u64]) -> ExitCode {
    let inventory = match parse_inventory_bytes(inventory_bytes) {
        Ok(inventory) => inventory,
        Err(error) => return fail_internal(&single_line(&error.to_string())),
    };
    for candidate in candidate_ids {
        let known = inventory
            .entries
            .iter()
            .any(|entry| entry.cache_id == *candidate);
        if !known {
            return fail_internal("cache_cleanup_candidate_unknown");
        }
    }
    ExitCode::SUCCESS
}

fn single_line(text: &str) -> String {
    text.replace(['\n', '\r'], " ")
}
