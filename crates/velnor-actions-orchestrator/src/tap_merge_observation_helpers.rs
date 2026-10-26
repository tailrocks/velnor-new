// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::{OrchestratorError, internal::internal};
use serde_json::Value;
use std::env;

pub(in crate::tap_merge) fn required_numeric(name: &str) -> Result<u64, OrchestratorError> {
    env::var(name)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .ok_or_else(|| internal("tap_merge_numeric_identity_missing"))
}

pub(in crate::tap_merge) fn required_string(name: &str) -> Result<String, OrchestratorError> {
    env::var(name)
        .ok()
        .filter(|value| !value.is_empty() && value.len() <= 128)
        .ok_or_else(|| internal("tap_merge_identity_missing"))
}

pub(in crate::tap_merge) fn valid_app_slug(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 39
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && !value.starts_with('-')
        && !value.ends_with('-')
}

pub(super) fn verify_repo_id(
    value: Option<&Value>,
    expected: u64,
) -> Result<(), OrchestratorError> {
    if value.and_then(Value::as_u64) == Some(expected) {
        Ok(())
    } else {
        Err(internal("tap_merge_target_repository_mismatch"))
    }
}

pub(in crate::tap_merge) fn encode_segment(value: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(char::from(byte));
        } else {
            encoded.push('%');
            encoded.push(char::from(HEX[usize::from(byte >> 4)]));
            encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
        }
    }
    encoded
}
