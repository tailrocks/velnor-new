// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    CC_ENV, CXX_ENV, MiseError, NATIVE_PREVIOUS_CC_ENV, NATIVE_PREVIOUS_CXX_ENV,
    NATIVE_QUERY_FLAGS, NATIVE_TOOL_KINDS, OPERATION_COMPILATION, OPERATION_COMPILER_QUERY,
    OsString, TOOL_KIND_CC, TOOL_KIND_CXX, reject, validate_adapter,
};

#[must_use]
pub fn is_supported_native_tool_kind(kind: &str) -> bool {
    NATIVE_TOOL_KINDS.contains(&kind)
}

#[must_use]
pub fn is_native_query_argv(argv: &[String]) -> bool {
    argv.iter()
        .any(|arg| NATIVE_QUERY_FLAGS.contains(&arg.as_str()))
}

#[must_use]
pub fn suggest_native_operation_kind(argv: &[String]) -> &'static str {
    if is_native_query_argv(argv) {
        OPERATION_COMPILER_QUERY
    } else {
        OPERATION_COMPILATION
    }
}

#[must_use]
pub fn is_observed_native_executable(
    executable: &str,
    cc_adapter: &str,
    cxx_adapter: &str,
) -> bool {
    executable == cc_adapter || executable == cxx_adapter
}

pub fn select_native_adapter<'a>(
    tool_kind: &str,
    cc_adapter: &'a str,
    cxx_adapter: &'a str,
) -> Result<&'a str, MiseError> {
    match tool_kind {
        TOOL_KIND_CC => {
            validate_adapter("cc_adapter", cc_adapter)?;
            Ok(cc_adapter)
        }
        TOOL_KIND_CXX => {
            validate_adapter("cxx_adapter", cxx_adapter)?;
            Ok(cxx_adapter)
        }
        _ => Err(reject("native_tool_kind", tool_kind)),
    }
}

pub fn resolve_native_tool_kind(
    executable: &str,
    cc_adapter: &str,
    cxx_adapter: &str,
) -> Result<&'static str, MiseError> {
    if cc_adapter == cxx_adapter {
        return Err(reject("native_adapter", cc_adapter));
    }
    if executable.is_empty() || executable.contains('\0') {
        return Err(reject("native_executable", executable));
    }
    if executable == cc_adapter {
        return Ok(TOOL_KIND_CC);
    }
    if executable == cxx_adapter {
        return Ok(TOOL_KIND_CXX);
    }
    Err(reject("native_executable", executable))
}

pub fn native_observer_env(
    cc_adapter: &str,
    cxx_adapter: &str,
    previous_cc: Option<&str>,
    previous_cxx: Option<&str>,
) -> Result<Vec<(OsString, OsString)>, MiseError> {
    validate_adapter("cc_adapter", cc_adapter)?;
    validate_adapter("cxx_adapter", cxx_adapter)?;
    if cc_adapter == cxx_adapter {
        return Err(reject("cxx_adapter", cxx_adapter));
    }
    if let Some(previous) = previous_cc {
        validate_adapter("previous_cc", previous)?;
    }
    if let Some(previous) = previous_cxx {
        validate_adapter("previous_cxx", previous)?;
    }
    let mut env = vec![
        (OsString::from(CC_ENV), OsString::from(cc_adapter)),
        (OsString::from(CXX_ENV), OsString::from(cxx_adapter)),
    ];
    if let Some(previous) = previous_cc {
        env.push((
            OsString::from(NATIVE_PREVIOUS_CC_ENV),
            OsString::from(previous),
        ));
    }
    if let Some(previous) = previous_cxx {
        env.push((
            OsString::from(NATIVE_PREVIOUS_CXX_ENV),
            OsString::from(previous),
        ));
    }
    Ok(env)
}

#[must_use]
pub fn native_chain_from_env(env: &[(OsString, OsString)]) -> Vec<String> {
    [
        CC_ENV,
        CXX_ENV,
        NATIVE_PREVIOUS_CC_ENV,
        NATIVE_PREVIOUS_CXX_ENV,
    ]
    .into_iter()
    .filter_map(|key| {
        env.iter()
            .rfind(|(found, _)| found == key)
            .map(|(_, value)| value.to_string_lossy().into_owned())
    })
    .filter(|value| !value.is_empty())
    .collect()
}
