// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;
use std::error::Error;
use std::ffi::OsString;

use velnor_actions_mise::IsolatedCommand;
use velnor_actions_mise::compiler_observer::{
    CompilerCompletion, CompilerEvent, CompilerLedger, DOC_PHASE_COMPILATION,
    DOC_PHASE_DOCTEST_DESCENDANT, DOC_PHASE_FINALIZATION, DOC_PHASE_PROBE,
    MBX_PREVIOUS_RUSTC_WRAPPER_ENV, MBX_TARGET_VIEWS_DISABLED, MBX_TARGET_VIEWS_ENV,
    OPERATION_COMPILER_QUERY, OPERATION_NONCOMPILATION, RUSTC_WRAPPER_ENV, RUSTDOC_ENV,
    classify_tool_kind, is_query_argv, observer_env, rustdoc_delegate_from_env, suggest_doc_phase,
    suggest_operation_kind, wrapper_chain_from_env,
};

fn check_records_rustc_start_and_completion() -> Result<(), Box<dyn Error>> {
    let invocation = dependency_invocation("event-rustc-1");
    invocation.validate()?;
    if classify_tool_kind(&invocation.executable)? != "rustc" {
        return Err(fail("rustc executable misclassified"));
    }
    if suggest_operation_kind(&invocation.argv) != OPERATION_COMPILATION {
        return Err(fail("compilation argv misclassified"));
    }
    let mut ledger = CompilerLedger::new();
    ledger.record(CompilerEvent {
        invocation,
        completion: None,
    })?;
    ledger.complete(
        "event-rustc-1",
        CompilerCompletion {
            exit_code: Some(0),
            signal: None,
            interrupted: false,
        },
    )?;
    if ledger.dependency_compilations() != 1 {
        return Err(fail("dependency compilation uncounted"));
    }
    if ledger.application_compilations() != 0 {
        return Err(fail("phantom application compilation counted"));
    }
    if ledger.failed() != 0 {
        return Err(fail("successful completion counted as failed"));
    }
    Ok(())
}

fn check_binds_recipe_and_package() -> Result<(), Box<dyn Error>> {
    let mut ledger = CompilerLedger::new();
    ledger.record(CompilerEvent {
        invocation: dependency_invocation("event-dep-1"),
        completion: Some(CompilerCompletion {
            exit_code: Some(0),
            signal: None,
            interrupted: false,
        }),
    })?;
    let mut application = dependency_invocation("event-app-1");
    "recipe-build-app".clone_into(&mut application.recipe_id);
    "velnor-actions-mise".clone_into(&mut application.package_id);
    ROLE_APPLICATION.clone_into(&mut application.dependency_role);
    ledger.record(CompilerEvent {
        invocation: application,
        completion: Some(CompilerCompletion {
            exit_code: Some(0),
            signal: None,
            interrupted: false,
        }),
    })?;
    if ledger.dependency_compilations() != 1 || ledger.application_compilations() != 1 {
        return Err(fail("recipe and package work not distinguished"));
    }
    Ok(())
}

fn check_distinguishes_documentation_phases() -> Result<(), Box<dyn Error>> {
    let phases = [
        (
            "event-doc-build",
            DOC_PHASE_COMPILATION,
            &["rustdoc", "--crate-name", "mise", "--output", "target/doc"] as &[&str],
        ),
        (
            "event-doc-finalize",
            DOC_PHASE_FINALIZATION,
            &["rustdoc", "--crate-name", "mise", "--output", "target/doc"] as &[&str],
        ),
        (
            "event-doc-probe",
            DOC_PHASE_PROBE,
            &["rustdoc", "--check", "--crate-name", "mise", "src/lib.rs"] as &[&str],
        ),
        (
            "event-doc-doctest",
            DOC_PHASE_DOCTEST_DESCENDANT,
            &["rustdoc", "--test", "--crate-name", "mise", "src/lib.rs"] as &[&str],
        ),
    ];
    for (event_id, phase, argv) in phases {
        rustdoc_invocation(event_id, phase, argv).validate()?;
    }
    if suggest_doc_phase(&[
        "rustdoc".to_owned(),
        "--test".to_owned(),
        "src/lib.rs".to_owned(),
    ]) != DOC_PHASE_DOCTEST_DESCENDANT
    {
        return Err(fail("doctest descendant unsuggested"));
    }
    if suggest_doc_phase(&[
        "rustdoc".to_owned(),
        "--check".to_owned(),
        "src/lib.rs".to_owned(),
    ]) != DOC_PHASE_PROBE
    {
        return Err(fail("probe unsuggested"));
    }
    if suggest_doc_phase(&[
        "rustdoc".to_owned(),
        "--crate-name".to_owned(),
        "mise".to_owned(),
    ]) != DOC_PHASE_COMPILATION
    {
        return Err(fail("documentation compilation unsuggested"));
    }
    if classify_tool_kind("/tools/rust/prefix/bin/rustdoc")? != "rustdoc" {
        return Err(fail("rustdoc executable misclassified"));
    }
    if classify_tool_kind("/tools/rust/prefix/bin/clippy-driver")? != "clippy-driver" {
        return Err(fail("clippy-driver executable misclassified"));
    }
    Ok(())
}

fn check_preserves_arguments_wrappers_signals_status() -> Result<(), Box<dyn Error>> {
    let env = vec![
        (
            OsString::from(RUSTC_WRAPPER_ENV),
            OsString::from("/opt/observer/rustc-observer"),
        ),
        (
            OsString::from(MBX_PREVIOUS_RUSTC_WRAPPER_ENV),
            OsString::from("/tools/mbx/bin/mbx-rustc"),
        ),
        (
            OsString::from(RUSTDOC_ENV),
            OsString::from("/opt/observer/rustdoc-observer"),
        ),
    ];
    let chain = wrapper_chain_from_env(&env);
    if chain
        != vec![
            "/opt/observer/rustc-observer".to_owned(),
            "/tools/mbx/bin/mbx-rustc".to_owned(),
        ]
    {
        return Err(fail("wrapper chain order not preserved"));
    }
    if rustdoc_delegate_from_env(&env).as_deref() != Some("/opt/observer/rustdoc-observer") {
        return Err(fail("rustdoc delegation lost"));
    }
    let mut invocation = dependency_invocation("event-signal-1");
    invocation.wrapper_chain.clone_from(&chain);
    let preserved_argv = invocation.argv.clone();
    invocation.validate()?;
    if invocation.argv != preserved_argv {
        return Err(fail("arguments mutated by validation"));
    }
    let signal = CompilerCompletion {
        exit_code: None,
        signal: Some(9),
        interrupted: false,
    };
    signal.validate()?;
    if signal.succeeded() {
        return Err(fail("signalled completion reported success"));
    }
    let interrupted = CompilerCompletion {
        exit_code: None,
        signal: None,
        interrupted: true,
    };
    interrupted.validate()?;
    let mut ledger = CompilerLedger::new();
    ledger.record(CompilerEvent {
        invocation,
        completion: Some(signal),
    })?;
    if ledger.failed() != 1 {
        return Err(fail("signalled compiler work uncounted"));
    }
    Ok(())
}

fn check_supports_previous_wrapper_and_rustdoc_delegation() -> Result<(), Box<dyn Error>> {
    let overlay = observer_env(
        "/opt/observer/rustc-observer",
        "/opt/observer/rustdoc-observer",
        Some("/tools/mbx/bin/mbx-rustc"),
        Some("/tools/mbx/bin/mbx-rustdoc"),
    )?;
    let get = |key: &str| {
        overlay
            .iter()
            .find(|(found, _)| found == key)
            .map(|(_, value)| value.to_string_lossy().into_owned())
    };
    if get(RUSTC_WRAPPER_ENV).as_deref() != Some("/opt/observer/rustc-observer") {
        return Err(fail("observer not installed as rustc wrapper"));
    }
    if get(MBX_PREVIOUS_RUSTC_WRAPPER_ENV).as_deref() != Some("/tools/mbx/bin/mbx-rustc") {
        return Err(fail("previous wrapper not preserved for MBX"));
    }
    if get(RUSTDOC_ENV).as_deref() != Some("/opt/observer/rustdoc-observer") {
        return Err(fail("owned rustdoc adapter not configured"));
    }
    if get(MBX_TARGET_VIEWS_ENV).as_deref() != Some(MBX_TARGET_VIEWS_DISABLED) {
        return Err(fail("target views not disabled for snapshot commands"));
    }
    let command = IsolatedCommand::repo_task("cargo", vec![OsString::from("build")], &[])?
        .with_compiler_observer(
            "/opt/observer/rustc-observer",
            "/opt/observer/rustdoc-observer",
            Some("/tools/mbx/bin/mbx-rustc"),
            None,
        )?;
    let full = command.full_env();
    let installed = |key: &str| {
        full.iter()
            .rfind(|(found, _)| found == key)
            .map(|(_, value)| value.to_string_lossy().into_owned())
    };
    if installed(RUSTC_WRAPPER_ENV).as_deref() != Some("/opt/observer/rustc-observer") {
        return Err(fail("command lost the observer wrapper"));
    }
    if installed(MBX_PREVIOUS_RUSTC_WRAPPER_ENV).as_deref() != Some("/tools/mbx/bin/mbx-rustc") {
        return Err(fail("command lost the previous wrapper delegation"));
    }
    if installed(MBX_TARGET_VIEWS_ENV).as_deref() != Some(MBX_TARGET_VIEWS_DISABLED) {
        return Err(fail("command lost the snapshot target-views setting"));
    }
    Ok(())
}

fn check_separates_queries_from_compilation() -> Result<(), Box<dyn Error>> {
    let query_argv = ["rustc".to_owned(), "--version".to_owned()];
    if !is_query_argv(&query_argv) {
        return Err(fail("version probe not recognized as query"));
    }
    if suggest_operation_kind(&query_argv) != OPERATION_COMPILER_QUERY {
        return Err(fail("query not suggested for version probe"));
    }
    let mut query = dependency_invocation("event-query-1");
    OPERATION_COMPILER_QUERY.clone_into(&mut query.operation_kind);
    query.validate()?;
    let mut noop = dependency_invocation("event-noncompilation-1");
    OPERATION_NONCOMPILATION.clone_into(&mut noop.operation_kind);
    noop.validate()?;
    let mut ledger = CompilerLedger::new();
    ledger.record(CompilerEvent {
        invocation: query,
        completion: None,
    })?;
    ledger.record(CompilerEvent {
        invocation: noop,
        completion: None,
    })?;
    if ledger.queries() != 1 {
        return Err(fail("compiler query counted as compilation"));
    }
    if ledger.dependency_compilations() != 0 {
        return Err(fail("query or noncompilation counted as compilation"));
    }
    Ok(())
}

#[test]
fn gc1_t17_preserves_contract() -> Result<(), Box<dyn Error>> {
    check_records_rustc_start_and_completion()?;
    check_binds_recipe_and_package()?;
    check_distinguishes_documentation_phases()?;
    check_preserves_arguments_wrappers_signals_status()?;
    check_supports_previous_wrapper_and_rustdoc_delegation()?;
    check_separates_queries_from_compilation()?;
    Ok(())
}
