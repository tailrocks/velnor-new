// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{MiseError, OsString};

pub const RUSTC_WRAPPER_ENV: &str = "RUSTC_WRAPPER";
pub const MBX_PREVIOUS_RUSTC_WRAPPER_ENV: &str = "MBX_PREVIOUS_RUSTC_WRAPPER";
pub const RUSTDOC_ENV: &str = "RUSTDOC";
pub const MBX_TARGET_VIEWS_ENV: &str = "MBX_TARGET_VIEWS";
pub const MBX_TARGET_VIEWS_DISABLED: &str = "0";

pub const TOOL_KIND_RUSTC: &str = "rustc";
pub const TOOL_KIND_CLIPPY_DRIVER: &str = "clippy-driver";
pub const TOOL_KIND_RUSTDOC: &str = "rustdoc";
pub const TOOL_KINDS: [&str; 3] = [TOOL_KIND_RUSTC, TOOL_KIND_CLIPPY_DRIVER, TOOL_KIND_RUSTDOC];

pub const OPERATION_COMPILATION: &str = "compilation";
pub const OPERATION_COMPILER_QUERY: &str = "compiler_query";
pub const OPERATION_NONCOMPILATION: &str = "noncompilation";
pub const OPERATION_KINDS: [&str; 3] = [
    OPERATION_COMPILATION,
    OPERATION_COMPILER_QUERY,
    OPERATION_NONCOMPILATION,
];

pub const DOC_PHASE_COMPILATION: &str = "doc_compilation";
pub const DOC_PHASE_FINALIZATION: &str = "finalization";
pub const DOC_PHASE_PROBE: &str = "probe";
pub const DOC_PHASE_DOCTEST_DESCENDANT: &str = "doctest_descendant";
pub const DOC_PHASES: [&str; 4] = [
    DOC_PHASE_COMPILATION,
    DOC_PHASE_FINALIZATION,
    DOC_PHASE_PROBE,
    DOC_PHASE_DOCTEST_DESCENDANT,
];

pub const ROLE_APPLICATION: &str = "application";
pub const ROLE_DEPENDENCY: &str = "dependency";
pub const DEPENDENCY_ROLES: [&str; 2] = [ROLE_APPLICATION, ROLE_DEPENDENCY];

pub const QUERY_FLAGS: [&str; 7] = [
    "--version",
    "-V",
    "-vV",
    "--help",
    "-h",
    "--print",
    "--explain",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompilerInvocation {
    pub event_id: String,
    pub recipe_id: String,
    pub package_id: String,
    pub dependency_role: String,
    pub lane_id: String,
    pub tool_kind: String,
    pub executable: String,
    pub argv: Vec<String>,
    pub wrapper_chain: Vec<String>,
    pub operation_kind: String,
    pub doc_phase: Option<String>,
    pub cwd: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompilerCompletion {
    pub exit_code: Option<i32>,
    pub signal: Option<i32>,
    pub interrupted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompilerEvent {
    pub invocation: CompilerInvocation,
    pub completion: Option<CompilerCompletion>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CompilerLedger {
    pub(crate) events: Vec<CompilerEvent>,
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

#[must_use]
pub fn is_supported_tool_kind(kind: &str) -> bool {
    TOOL_KINDS.contains(&kind)
}

#[must_use]
pub fn is_supported_operation_kind(kind: &str) -> bool {
    OPERATION_KINDS.contains(&kind)
}

#[must_use]
pub fn is_supported_doc_phase(phase: &str) -> bool {
    DOC_PHASES.contains(&phase)
}

#[must_use]
pub fn is_supported_dependency_role(role: &str) -> bool {
    DEPENDENCY_ROLES.contains(&role)
}

#[must_use]
pub fn is_query_argv(argv: &[String]) -> bool {
    argv.iter().any(|arg| QUERY_FLAGS.contains(&arg.as_str()))
}

#[must_use]
pub fn suggest_operation_kind(argv: &[String]) -> &'static str {
    if is_query_argv(argv) {
        OPERATION_COMPILER_QUERY
    } else {
        OPERATION_COMPILATION
    }
}

#[must_use]
pub fn suggest_doc_phase(argv: &[String]) -> &'static str {
    if argv.iter().any(|arg| arg == "--test") {
        DOC_PHASE_DOCTEST_DESCENDANT
    } else if argv.iter().any(|arg| arg == "--check") {
        DOC_PHASE_PROBE
    } else {
        DOC_PHASE_COMPILATION
    }
}

pub fn classify_tool_kind(executable: &str) -> Result<&'static str, MiseError> {
    let base = executable.rsplit(['/', '\\']).next().unwrap_or(executable);
    let stem = base.strip_suffix(".exe").unwrap_or(base);
    match stem {
        TOOL_KIND_RUSTC => Ok(TOOL_KIND_RUSTC),
        TOOL_KIND_CLIPPY_DRIVER => Ok(TOOL_KIND_CLIPPY_DRIVER),
        TOOL_KIND_RUSTDOC => Ok(TOOL_KIND_RUSTDOC),
        _ => Err(reject("tool_kind", executable)),
    }
}

#[must_use]
pub fn wrapper_chain_from_env(env: &[(OsString, OsString)]) -> Vec<String> {
    [RUSTC_WRAPPER_ENV, MBX_PREVIOUS_RUSTC_WRAPPER_ENV]
        .into_iter()
        .filter_map(|key| {
            env.iter()
                .rfind(|(found, _)| found == key)
                .map(|(_, value)| value.to_string_lossy().into_owned())
        })
        .filter(|value| !value.is_empty())
        .collect()
}

#[must_use]
pub fn rustdoc_delegate_from_env(env: &[(OsString, OsString)]) -> Option<String> {
    env.iter()
        .rfind(|(found, _)| found == RUSTDOC_ENV)
        .map(|(_, value)| value.to_string_lossy().into_owned())
        .filter(|value| !value.is_empty())
}

pub fn observer_env(
    observer: &str,
    rustdoc_observer: &str,
    previous_rustc_wrapper: Option<&str>,
    previous_rustdoc: Option<&str>,
) -> Result<Vec<(OsString, OsString)>, MiseError> {
    if observer.is_empty() || observer.contains('\0') {
        return Err(reject("rustc_observer", observer));
    }
    if rustdoc_observer.is_empty() || rustdoc_observer.contains('\0') {
        return Err(reject("rustdoc_observer", rustdoc_observer));
    }
    if previous_rustc_wrapper.is_some_and(|value| value.is_empty() || value.contains('\0')) {
        return Err(reject(
            "previous_rustc_wrapper",
            previous_rustc_wrapper.unwrap_or(""),
        ));
    }
    if previous_rustdoc.is_some_and(|value| value.is_empty() || value.contains('\0')) {
        return Err(reject("previous_rustdoc", previous_rustdoc.unwrap_or("")));
    }
    let mut env = vec![
        (OsString::from(RUSTC_WRAPPER_ENV), OsString::from(observer)),
        (
            OsString::from(RUSTDOC_ENV),
            OsString::from(rustdoc_observer),
        ),
        (
            OsString::from(MBX_TARGET_VIEWS_ENV),
            OsString::from(MBX_TARGET_VIEWS_DISABLED),
        ),
    ];
    if let Some(previous) = previous_rustc_wrapper {
        env.push((
            OsString::from(MBX_PREVIOUS_RUSTC_WRAPPER_ENV),
            OsString::from(previous),
        ));
    }
    if let Some(previous) = previous_rustdoc {
        env.push((
            OsString::from("MBX_PREVIOUS_RUSTDOC"),
            OsString::from(previous),
        ));
    }
    Ok(env)
}
