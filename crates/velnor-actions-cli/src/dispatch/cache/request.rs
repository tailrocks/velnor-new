// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_orchestrator::cache_recovery::MAX_RECOVERY_TEXT_BYTES;

use super::{
    CACHE_SCHEMA, COMMAND_KEYS, CacheOp, Expected, MAX_CACHE_REQUEST_BYTES, MAX_CLEANUP_CANDIDATES,
    ValidatedCacheRequest,
};

pub(super) fn validate_request(op: CacheOp, bytes: &[u8]) -> Result<ValidatedCacheRequest, String> {
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
        expected_bytes: required_object(op, body, "expected")?,
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
        CacheOp::Publish => matches!(
            key,
            "schema" | "operation" | "admission" | "inventory" | "expected"
        ),
        CacheOp::Recover => matches!(key, "schema" | "operation" | "inventory" | "expected"),
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

pub(super) fn parse_expected(bytes: &[u8]) -> Result<Expected, String> {
    let value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|_| "cache_expected_malformed".to_owned())?;
    let body = value
        .as_object()
        .ok_or_else(|| "cache_expected_malformed".to_owned())?;
    for key in body.keys() {
        if !matches!(key.as_str(), "key" | "ref" | "version") {
            return Err("cache_expected_field_rejected".to_owned());
        }
    }
    let field = |name: &str| {
        body.get(name)
            .and_then(serde_json::Value::as_str)
            .filter(|value| {
                !value.is_empty()
                    && value.len() <= MAX_RECOVERY_TEXT_BYTES
                    && !value.bytes().any(|byte| byte == 0)
            })
            .map(str::to_owned)
            .ok_or_else(|| format!("cache_expected_{name}_malformed"))
    };
    Ok(Expected {
        key: field("key")?,
        ref_name: field("ref")?,
        version: field("version")?,
    })
}
