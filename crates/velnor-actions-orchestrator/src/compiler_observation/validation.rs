// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::workflow::cache_context::MeasuredU64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ObservationError {
    code: &'static str,
    detail: String,
}

impl std::fmt::Display for ObservationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.detail)
    }
}

impl std::error::Error for ObservationError {}

pub(super) fn reject(field: &'static str, value: &str) -> ObservationError {
    ObservationError {
        code: field,
        detail: value.to_owned(),
    }
}

pub(super) fn check_binding(field: &'static str, value: &str) -> Result<(), ObservationError> {
    if value.is_empty() {
        return Err(reject(field, "empty"));
    }
    if value.contains('\0') {
        return Err(reject(field, "nul_byte"));
    }
    if value.contains("..") {
        return Err(reject(field, "dotdot"));
    }
    Ok(())
}

pub(super) fn check_session_id(value: &str) -> Result<(), ObservationError> {
    check_binding("session_id", value)?;
    if value.contains('/') || value.contains('\\') {
        return Err(reject("session_id", "separator"));
    }
    Ok(())
}

pub(super) fn check_context(value: &str) -> Result<(), ObservationError> {
    if value.is_empty() {
        return Err(reject("execution_context_digest", "empty"));
    }
    if value.contains('\0') {
        return Err(reject("execution_context_digest", "nul_byte"));
    }
    Ok(())
}

pub(super) fn check_detail(value: &str) -> Result<(), ObservationError> {
    if value.is_empty() {
        return Err(reject("unknown_detail", "empty"));
    }
    if value.contains('\0') {
        return Err(reject("unknown_detail", "nul_byte"));
    }
    Ok(())
}

pub(super) fn measured(value: u64) -> MeasuredU64 {
    MeasuredU64 {
        value: Some(value),
        unknown_reason: None,
    }
}
