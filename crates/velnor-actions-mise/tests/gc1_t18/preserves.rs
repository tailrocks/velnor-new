// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;
use std::error::Error;
use std::ffi::OsString;

use velnor_actions_mise::IsolatedCommand;
use velnor_actions_mise::compiler_observer::{
    CompilerCompletion, CompilerEvent, CompilerLedger, OPERATION_COMPILER_QUERY,
    OPERATION_NONCOMPILATION, RUSTC_WRAPPER_ENV, compose_observer_env, observer_env,
};
use velnor_actions_mise::native_compiler_observer::{
    CC_ENV, CXX_ENV, NATIVE_PREVIOUS_CC_ENV, NATIVE_PREVIOUS_CXX_ENV, NativeCompletion,
    NativeEvent, NativeLedger, TOOL_KIND_CC, TOOL_KIND_CXX, is_native_query_argv,
    is_observed_native_executable, is_supported_native_tool_kind, native_chain_from_env,
    native_observer_env, resolve_native_tool_kind, select_native_adapter,
    suggest_native_operation_kind,
};

fn check_wraps_cc_cxx_at_stable_paths() -> Result<(), Box<dyn Error>> {
    let env = native_observer_env(
        CC_ADAPTER,
        CXX_ADAPTER,
        Some(PREVIOUS_CC),
        Some(PREVIOUS_CXX),
    )?;
    let get = |key: &str| {
        env.iter()
            .find(|(found, _)| found == key)
            .map(|(_, value)| value.to_string_lossy().into_owned())
    };
    if get(CC_ENV).as_deref() != Some(CC_ADAPTER) {
        return Err(fail("CC not bound to the stable adapter"));
    }
    if get(CXX_ENV).as_deref() != Some(CXX_ADAPTER) {
        return Err(fail("CXX not bound to the stable adapter"));
    }
    if get(NATIVE_PREVIOUS_CC_ENV).as_deref() != Some(PREVIOUS_CC) {
        return Err(fail("previous CC executable not preserved"));
    }
    if get(NATIVE_PREVIOUS_CXX_ENV).as_deref() != Some(PREVIOUS_CXX) {
        return Err(fail("previous CXX executable not preserved"));
    }
    let chain = native_chain_from_env(&env);
    if chain
        != vec![
            CC_ADAPTER.to_owned(),
            CXX_ADAPTER.to_owned(),
            PREVIOUS_CC.to_owned(),
            PREVIOUS_CXX.to_owned(),
        ]
    {
        return Err(fail("native adapter chain order not preserved"));
    }
    if select_native_adapter(TOOL_KIND_CC, CC_ADAPTER, CXX_ADAPTER)? != CC_ADAPTER {
        return Err(fail("CC selection lost its adapter"));
    }
    if select_native_adapter(TOOL_KIND_CXX, CC_ADAPTER, CXX_ADAPTER)? != CXX_ADAPTER {
        return Err(fail("CXX selection lost its adapter"));
    }
    if !is_supported_native_tool_kind(TOOL_KIND_CC) || !is_supported_native_tool_kind(TOOL_KIND_CXX)
    {
        return Err(fail("approved native tool kinds unsupported"));
    }
    if is_supported_native_tool_kind("gcc") {
        return Err(fail("raw compiler name admitted as tool kind"));
    }
    Ok(())
}

fn check_preserves_flags_and_selection() -> Result<(), Box<dyn Error>> {
    let invocation = native_invocation("event-cc-1", TOOL_KIND_CC, CC_ADAPTER);
    invocation.validate()?;
    let preserved_argv = invocation.argv.clone();
    if invocation.argv != preserved_argv {
        return Err(fail("native flags mutated by validation"));
    }
    if !preserved_argv.contains(&"-DFOO=bar baz".to_owned()) {
        return Err(fail("native flag with embedded space lost"));
    }
    if resolve_native_tool_kind(CC_ADAPTER, CC_ADAPTER, CXX_ADAPTER)? != TOOL_KIND_CC {
        return Err(fail("CC adapter call misclassified"));
    }
    if resolve_native_tool_kind(CXX_ADAPTER, CC_ADAPTER, CXX_ADAPTER)? != TOOL_KIND_CXX {
        return Err(fail("CXX adapter call misclassified"));
    }
    if invocation.executable
        != select_native_adapter(&invocation.tool_kind, CC_ADAPTER, CXX_ADAPTER)?
    {
        return Err(fail("executable diverged from tool selection"));
    }
    Ok(())
}

fn check_records_native_start_and_completion() -> Result<(), Box<dyn Error>> {
    let mut ledger = NativeLedger::new();
    if !ledger.is_empty() {
        return Err(fail("fresh native ledger not empty"));
    }
    ledger.record(NativeEvent {
        invocation: native_invocation("event-cc-1", TOOL_KIND_CC, CC_ADAPTER),
        completion: None,
    })?;
    ledger.complete(
        "event-cc-1",
        NativeCompletion {
            exit_code: Some(0),
            signal: None,
            interrupted: false,
        },
    )?;
    if ledger.dependency_compilations() != 1 {
        return Err(fail("native dependency compilation uncounted"));
    }
    if ledger.application_compilations() != 0 {
        return Err(fail("phantom native application compilation counted"));
    }
    let mut application = native_invocation("event-cxx-1", TOOL_KIND_CXX, CXX_ADAPTER);
    ROLE_APPLICATION.clone_into(&mut application.dependency_role);
    ledger.record(NativeEvent {
        invocation: application,
        completion: Some(NativeCompletion {
            exit_code: Some(0),
            signal: None,
            interrupted: false,
        }),
    })?;
    if ledger.native_compilations() != 2 {
        return Err(fail("observed native work miscounted"));
    }
    if ledger.measured_native_compilations() != Some(2) {
        return Err(fail("complete observation did not measure native work"));
    }
    if ledger.failed() != 0 {
        return Err(fail("successful native completion counted as failed"));
    }
    Ok(())
}

fn check_nests_failures_and_cancellations() -> Result<(), Box<dyn Error>> {
    let mut rust_ledger = CompilerLedger::new();
    rust_ledger.record(CompilerEvent {
        invocation: parent_rustc_invocation(),
        completion: Some(CompilerCompletion {
            exit_code: Some(0),
            signal: None,
            interrupted: false,
        }),
    })?;
    let mut ledger = NativeLedger::new();
    let mut failed_child = native_invocation("event-cc-nested-fail", TOOL_KIND_CC, CC_ADAPTER);
    failed_child.parent_event_id = Some("event-parent-rustc".to_owned());
    failed_child.validate()?;
    ledger.record(NativeEvent {
        invocation: failed_child,
        completion: None,
    })?;
    ledger.complete(
        "event-cc-nested-fail",
        NativeCompletion {
            exit_code: Some(1),
            signal: None,
            interrupted: false,
        },
    )?;
    let mut cancelled_child =
        native_invocation("event-cxx-nested-cancel", TOOL_KIND_CXX, CXX_ADAPTER);
    cancelled_child.parent_event_id = Some("event-parent-rustc".to_owned());
    ledger.record(NativeEvent {
        invocation: cancelled_child,
        completion: Some(NativeCompletion {
            exit_code: None,
            signal: None,
            interrupted: true,
        }),
    })?;
    if ledger.failed() != 2 {
        return Err(fail("nested native failure uncounted"));
    }
    if ledger.interrupted() != 1 {
        return Err(fail("nested native cancellation uncounted"));
    }
    if ledger.measured_native_compilations() != Some(2) {
        return Err(fail("nested native work mismeasured"));
    }
    if rust_ledger.dependency_compilations() != 0 {
        return Err(fail("parent rustc compilation misattributed"));
    }
    Ok(())
}

fn check_separates_native_queries() -> Result<(), Box<dyn Error>> {
    let query_argv = ["c++".to_owned(), "--version".to_owned()];
    if !is_native_query_argv(&query_argv) {
        return Err(fail("native version probe not recognized as query"));
    }
    if suggest_native_operation_kind(&query_argv) != OPERATION_COMPILER_QUERY {
        return Err(fail("query not suggested for native version probe"));
    }
    let mut ledger = NativeLedger::new();
    let mut query = native_invocation("event-query-1", TOOL_KIND_CXX, CXX_ADAPTER);
    OPERATION_COMPILER_QUERY.clone_into(&mut query.operation_kind);
    query.validate()?;
    ledger.record(NativeEvent {
        invocation: query,
        completion: None,
    })?;
    let mut noop = native_invocation("event-noncompilation-1", TOOL_KIND_CC, CC_ADAPTER);
    OPERATION_NONCOMPILATION.clone_into(&mut noop.operation_kind);
    noop.validate()?;
    ledger.record(NativeEvent {
        invocation: noop,
        completion: None,
    })?;
    if ledger.queries() != 1 {
        return Err(fail("native query counted as compilation"));
    }
    if ledger.native_compilations() != 0 {
        return Err(fail(
            "query or noncompilation counted as native compilation",
        ));
    }
    if ledger.measured_native_compilations() != Some(0) {
        return Err(fail("complete empty observation did not measure zero"));
    }
    Ok(())
}

fn check_composes_with_rust_observer_env() -> Result<(), Box<dyn Error>> {
    let rust = observer_env(
        "/opt/observer/rustc-observer",
        "/opt/observer/rustdoc-observer",
        Some("/tools/mbx/bin/mbx-rustc"),
        None,
    )?;
    let native = native_observer_env(CC_ADAPTER, CXX_ADAPTER, Some(PREVIOUS_CC), None)?;
    let merged = compose_observer_env(rust, native)?;
    let get = |key: &str| {
        merged
            .iter()
            .find(|(found, _)| found == key)
            .map(|(_, value)| value.to_string_lossy().into_owned())
    };
    if get(RUSTC_WRAPPER_ENV).as_deref() != Some("/opt/observer/rustc-observer") {
        return Err(fail("rust observer lost in composition"));
    }
    if get(CC_ENV).as_deref() != Some(CC_ADAPTER) {
        return Err(fail("CC adapter lost in composition"));
    }
    if get(CXX_ENV).as_deref() != Some(CXX_ADAPTER) {
        return Err(fail("CXX adapter lost in composition"));
    }
    if get(NATIVE_PREVIOUS_CC_ENV).as_deref() != Some(PREVIOUS_CC) {
        return Err(fail("previous CC lost in composition"));
    }
    let command = IsolatedCommand::repo_task("cargo", vec![OsString::from("build")], &[])?
        .with_env(&merged)?;
    let full = command.full_env();
    let installed = |key: &str| {
        full.iter()
            .rfind(|(found, _)| found == key)
            .map(|(_, value)| value.to_string_lossy().into_owned())
    };
    if installed(RUSTC_WRAPPER_ENV).as_deref() != Some("/opt/observer/rustc-observer") {
        return Err(fail("command lost the rust observer wrapper"));
    }
    if installed(CC_ENV).as_deref() != Some(CC_ADAPTER) {
        return Err(fail("command lost the CC adapter"));
    }
    if installed(CXX_ENV).as_deref() != Some(CXX_ADAPTER) {
        return Err(fail("command lost the CXX adapter"));
    }
    Ok(())
}

fn check_marks_unobserved_as_unknown() -> Result<(), Box<dyn Error>> {
    if resolve_native_tool_kind(PREVIOUS_CC, CC_ADAPTER, CXX_ADAPTER).is_ok() {
        return Err(fail("absolute bypassing CC call classified as observed"));
    }
    if resolve_native_tool_kind(PREVIOUS_CXX, CC_ADAPTER, CXX_ADAPTER).is_ok() {
        return Err(fail("absolute bypassing CXX call classified as observed"));
    }
    if resolve_native_tool_kind("cc", CC_ADAPTER, CXX_ADAPTER).is_ok() {
        return Err(fail("bare compiler name classified as observed"));
    }
    if is_observed_native_executable(PREVIOUS_CC, CC_ADAPTER, CXX_ADAPTER) {
        return Err(fail("bypassing executable reported as observed"));
    }
    if !is_observed_native_executable(CC_ADAPTER, CC_ADAPTER, CXX_ADAPTER) {
        return Err(fail("adapter-routed call reported as unobserved"));
    }
    let mut ledger = NativeLedger::new();
    ledger.record(NativeEvent {
        invocation: native_invocation("event-cc-1", TOOL_KIND_CC, CC_ADAPTER),
        completion: Some(NativeCompletion {
            exit_code: Some(0),
            signal: None,
            interrupted: false,
        }),
    })?;
    if ledger.measured_native_compilations() != Some(1) {
        return Err(fail("observed native work mismeasured before gap"));
    }
    ledger.record_unobserved_absolute(PREVIOUS_CC)?;
    if ledger.unobserved_absolute_calls() != 1 {
        return Err(fail("unobserved absolute call uncounted"));
    }
    if ledger.measured_native_compilations().is_some() {
        return Err(fail("unsupported observation passed as a measurement"));
    }
    Ok(())
}

#[test]
fn gc1_t18_preserves_contract() -> Result<(), Box<dyn Error>> {
    check_wraps_cc_cxx_at_stable_paths()?;
    check_preserves_flags_and_selection()?;
    check_records_native_start_and_completion()?;
    check_nests_failures_and_cancellations()?;
    check_separates_native_queries()?;
    check_composes_with_rust_observer_env()?;
    check_marks_unobserved_as_unknown()?;
    Ok(())
}
