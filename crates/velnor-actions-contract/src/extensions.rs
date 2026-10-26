// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::cachekey::validate_semantic_text;
use crate::cachekey::{RUST_EXTENSION_SCHEMA, TOFU_EXTENSION_SCHEMA};
use crate::canonical::{StackExtension, validate_digest};
use crate::errors::ContractError;

pub const RUST_EXTENSION_REQUIRED_SLOTS: [&str; 11] = [
    "package_id",
    "workspace_id",
    "graph_digest",
    "targets",
    "features",
    "profile",
    "driver",
    "config_digest",
    "nextest_digest",
    "kind",
    "archive",
];

pub fn validate_rust_extension(extension: &StackExtension) -> Result<(), ContractError> {
    if extension.schema != RUST_EXTENSION_SCHEMA {
        return Err(ContractError::identity(
            "stack_extension.schema",
            "unknown_schema",
        ));
    }
    let Some(data) = extension.data.as_object() else {
        return Err(ContractError::identity(
            "stack_extension.data",
            "must_be_object",
        ));
    };
    for slot in RUST_EXTENSION_REQUIRED_SLOTS {
        if !data.contains_key(slot) {
            return Err(ContractError::identity(
                "stack_extension.data",
                format!("missing_slot:{slot}"),
            ));
        }
    }
    check_extension_text(data, "package_id")?;
    check_extension_text(data, "workspace_id")?;
    check_extension_text(data, "profile")?;
    check_extension_text(data, "kind")?;
    check_driver_slot(data)?;
    validate_digest(slot_str(data, "graph_digest"))?;
    validate_digest(slot_str(data, "config_digest"))?;
    check_string_array(data, "targets")?;
    check_string_array(data, "features")?;
    if data
        .get("all_features")
        .is_some_and(|value| !value.is_boolean())
    {
        return Err(ContractError::identity(
            "stack_extension.data.all_features",
            "must_be_boolean",
        ));
    }
    check_nullable_text(data, "nextest_digest")?;
    check_nullable_text(data, "archive")?;
    Ok(())
}

pub const TOFU_EXTENSION_REQUIRED_SLOTS: [&str; 9] = [
    "unit_id",
    "workspace_id",
    "graph_digest",
    "root",
    "profile",
    "driver",
    "config_digest",
    "lock_digest",
    "kind",
];

pub fn validate_tofu_extension(extension: &StackExtension) -> Result<(), ContractError> {
    if extension.schema != TOFU_EXTENSION_SCHEMA {
        return Err(ContractError::identity(
            "stack_extension.schema",
            "unknown_schema",
        ));
    }
    let Some(data) = extension.data.as_object() else {
        return Err(ContractError::identity(
            "stack_extension.data",
            "must_be_object",
        ));
    };
    for slot in TOFU_EXTENSION_REQUIRED_SLOTS {
        if !data.contains_key(slot) {
            return Err(ContractError::identity(
                "stack_extension.data",
                format!("missing_slot:{slot}"),
            ));
        }
    }
    check_extension_text(data, "unit_id")?;
    check_extension_text(data, "workspace_id")?;
    check_extension_text(data, "profile")?;
    check_extension_text(data, "kind")?;
    check_root_slot(data)?;
    check_driver_slot(data)?;
    validate_digest(slot_str(data, "graph_digest"))?;
    validate_digest(slot_str(data, "config_digest"))?;
    check_nullable_text(data, "lock_digest")?;
    Ok(())
}

fn check_root_slot(data: &serde_json::Map<String, serde_json::Value>) -> Result<(), ContractError> {
    let root = slot_str(data, "root");
    if root.is_empty() {
        return Ok(());
    }
    validate_semantic_text("stack_extension.data", root)?;
    Ok(())
}

fn slot_str<'a>(data: &'a serde_json::Map<String, serde_json::Value>, slot: &str) -> &'a str {
    data.get(slot)
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
}

fn check_extension_text(
    data: &serde_json::Map<String, serde_json::Value>,
    slot: &str,
) -> Result<(), ContractError> {
    let value = slot_str(data, slot);
    if value.is_empty() {
        return Err(ContractError::identity(
            "stack_extension.data",
            format!("empty_slot:{slot}"),
        ));
    }
    validate_semantic_text("stack_extension.data", value)?;
    Ok(())
}

fn check_driver_slot(
    data: &serde_json::Map<String, serde_json::Value>,
) -> Result<(), ContractError> {
    let driver = slot_str(data, "driver");
    if driver.is_empty() || !driver.contains('+') {
        return Err(ContractError::identity(
            "stack_extension.data",
            "driver_must_combine_driver_and_runner",
        ));
    }
    for part in driver.split('+') {
        validate_semantic_text("stack_extension.driver", part)?;
    }
    Ok(())
}

fn check_string_array(
    data: &serde_json::Map<String, serde_json::Value>,
    slot: &str,
) -> Result<(), ContractError> {
    let Some(values) = data.get(slot).and_then(serde_json::Value::as_array) else {
        return Err(ContractError::identity(
            "stack_extension.data",
            format!("bad_slot:{slot}"),
        ));
    };
    for value in values {
        let Some(text) = value.as_str() else {
            return Err(ContractError::identity(
                "stack_extension.data",
                format!("bad_slot:{slot}"),
            ));
        };
        validate_semantic_text("stack_extension.data", text)?;
    }
    Ok(())
}

fn check_nullable_text(
    data: &serde_json::Map<String, serde_json::Value>,
    slot: &str,
) -> Result<(), ContractError> {
    match data.get(slot) {
        None | Some(serde_json::Value::Null) => Ok(()),
        Some(serde_json::Value::String(text)) => {
            validate_semantic_text("stack_extension.data", text)?;
            Ok(())
        }
        Some(_) => Err(ContractError::identity(
            "stack_extension.data",
            format!("bad_slot:{slot}"),
        )),
    }
}
