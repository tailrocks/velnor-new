// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::MiseError;

pub const CC_ENV: &str = "CC";
pub const CXX_ENV: &str = "CXX";
pub const NATIVE_PREVIOUS_CC_ENV: &str = "VELNOR_PREVIOUS_CC";
pub const NATIVE_PREVIOUS_CXX_ENV: &str = "VELNOR_PREVIOUS_CXX";

pub const TOOL_KIND_CC: &str = "CC";
pub const TOOL_KIND_CXX: &str = "CXX";
pub const NATIVE_TOOL_KINDS: [&str; 2] = [TOOL_KIND_CC, TOOL_KIND_CXX];

pub const NATIVE_QUERY_FLAGS: [&str; 6] = [
    "--version",
    "-v",
    "--help",
    "-h",
    "-dumpversion",
    "-dumpmachine",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeInvocation {
    pub event_id: String,
    pub recipe_id: String,
    pub package_id: String,
    pub dependency_role: String,
    pub lane_id: String,
    pub tool_kind: String,
    pub executable: String,
    pub argv: Vec<String>,
    pub adapter_chain: Vec<String>,
    pub parent_event_id: Option<String>,
    pub operation_kind: String,
    pub cwd: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeCompletion {
    pub exit_code: Option<i32>,
    pub signal: Option<i32>,
    pub interrupted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeEvent {
    pub invocation: NativeInvocation,
    pub completion: Option<NativeCompletion>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NativeLedger {
    pub(crate) events: Vec<NativeEvent>,
    pub(crate) unobserved_absolute_calls: u64,
}

pub(crate) fn reject(field: &'static str, value: &str) -> MiseError {
    MiseError::InvalidStepInput {
        field: field.to_owned(),
        value: value.to_owned(),
    }
}

pub(crate) fn validate_id(field: &'static str, value: &str) -> Result<(), MiseError> {
    if value.is_empty() || value.contains('\0') || value.contains("..") {
        return Err(reject(field, value));
    }
    Ok(())
}

pub(crate) fn validate_lane(lane: &str) -> Result<(), MiseError> {
    validate_id("lane_id", lane)?;
    if lane == "." || lane.contains('/') || lane.contains('\\') {
        return Err(reject("lane_id", lane));
    }
    Ok(())
}

pub(crate) fn validate_adapter(field: &'static str, value: &str) -> Result<(), MiseError> {
    if value.is_empty() || value.contains('\0') || !value.starts_with('/') {
        return Err(reject(field, value));
    }
    Ok(())
}
