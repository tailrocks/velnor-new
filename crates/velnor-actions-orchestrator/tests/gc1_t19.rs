// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[path = "../src/compiler_observation.rs"]
mod compiler_observation;

use std::error::Error;
use std::io;

use self::compiler_observation::{
    ChildOutcome, ObservationReport, ObservationSession, new_session_id,
};
use velnor_actions_contract::digest_b3;
use velnor_actions_contract::workflow::cache_context::{
    MeasuredU64, ObservationGapCode, ObservationGapV1,
};
use velnor_actions_mise::compiler_observer::{
    CompilerCompletion, CompilerEvent, CompilerInvocation, CompilerLedger, OPERATION_COMPILATION,
    OPERATION_COMPILER_QUERY, ROLE_APPLICATION, ROLE_DEPENDENCY, TOOL_KIND_RUSTC,
};
use velnor_actions_mise::native_compiler_observer::{
    NativeCompletion, NativeEvent, NativeInvocation, NativeLedger, TOOL_KIND_CC, TOOL_KIND_CXX,
};

fn fail(message: &str) -> Box<dyn Error> {
    io::Error::other(message).into()
}

fn context_digest() -> String {
    digest_b3(b"gc1-t19-observation-root")
}

fn valid_session() -> Result<ObservationSession, Box<dyn Error>> {
    Ok(ObservationSession::begin(
        "internal/lint/dev",
        "gc1-t19-check",
        &new_session_id(),
        &context_digest(),
    )?)
}

fn rust_argv() -> Vec<String> {
    [
        "rustc",
        "--crate-name",
        "mylib",
        "--edition=2021",
        "src/lib.rs",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

fn rust_invocation(event_id: &str, role: &str, operation: &str) -> CompilerInvocation {
    CompilerInvocation {
        event_id: event_id.to_owned(),
        recipe_id: "internal/lint/dev".to_owned(),
        package_id: "mylib".to_owned(),
        dependency_role: role.to_owned(),
        lane_id: "build".to_owned(),
        tool_kind: TOOL_KIND_RUSTC.to_owned(),
        executable: "/tools/rust/prefix/bin/rustc".to_owned(),
        argv: rust_argv(),
        wrapper_chain: Vec::new(),
        operation_kind: operation.to_owned(),
        doc_phase: None,
        cwd: "/repo".to_owned(),
    }
}

fn rust_success() -> CompilerCompletion {
    CompilerCompletion {
        exit_code: Some(0),
        signal: None,
        interrupted: false,
    }
}

fn rust_ledger() -> Result<CompilerLedger, Box<dyn Error>> {
    let mut ledger = CompilerLedger::new();
    ledger.record(CompilerEvent {
        invocation: rust_invocation("evt-dep-1", ROLE_DEPENDENCY, OPERATION_COMPILATION),
        completion: None,
    })?;
    ledger.complete("evt-dep-1", rust_success())?;
    ledger.record(CompilerEvent {
        invocation: rust_invocation("evt-dep-2", ROLE_DEPENDENCY, OPERATION_COMPILATION),
        completion: None,
    })?;
    ledger.complete(
        "evt-dep-2",
        CompilerCompletion {
            exit_code: Some(101),
            signal: None,
            interrupted: false,
        },
    )?;
    ledger.record(CompilerEvent {
        invocation: rust_invocation("evt-dep-3", ROLE_DEPENDENCY, OPERATION_COMPILATION),
        completion: None,
    })?;
    ledger.complete(
        "evt-dep-3",
        CompilerCompletion {
            exit_code: None,
            signal: None,
            interrupted: true,
        },
    )?;
    ledger.record(CompilerEvent {
        invocation: rust_invocation("evt-app-1", ROLE_APPLICATION, OPERATION_COMPILATION),
        completion: None,
    })?;
    ledger.complete("evt-app-1", rust_success())?;
    ledger.record(CompilerEvent {
        invocation: rust_invocation("evt-query-1", ROLE_APPLICATION, OPERATION_COMPILER_QUERY),
        completion: None,
    })?;
    ledger.complete("evt-query-1", rust_success())?;
    Ok(ledger)
}

fn native_argv() -> Vec<String> {
    ["cc", "-O2", "-c", "src/native.c", "-o", "out/native.o"]
        .into_iter()
        .map(str::to_owned)
        .collect()
}

fn native_invocation(
    event_id: &str,
    tool_kind: &str,
    executable: &str,
    operation: &str,
) -> NativeInvocation {
    NativeInvocation {
        event_id: event_id.to_owned(),
        recipe_id: "internal/lint/dev".to_owned(),
        package_id: "libnative".to_owned(),
        dependency_role: ROLE_DEPENDENCY.to_owned(),
        lane_id: "build".to_owned(),
        tool_kind: tool_kind.to_owned(),
        executable: executable.to_owned(),
        argv: native_argv(),
        adapter_chain: vec![executable.to_owned()],
        parent_event_id: None,
        operation_kind: operation.to_owned(),
        cwd: "/repo".to_owned(),
    }
}

fn native_success() -> NativeCompletion {
    NativeCompletion {
        exit_code: Some(0),
        signal: None,
        interrupted: false,
    }
}

fn native_ledger() -> Result<NativeLedger, Box<dyn Error>> {
    let mut ledger = NativeLedger::new();
    ledger.record(NativeEvent {
        invocation: native_invocation(
            "evt-cc-1",
            TOOL_KIND_CC,
            "/tools/native/cc",
            OPERATION_COMPILATION,
        ),
        completion: None,
    })?;
    ledger.complete("evt-cc-1", native_success())?;
    ledger.record(NativeEvent {
        invocation: native_invocation(
            "evt-cxx-query-1",
            TOOL_KIND_CXX,
            "/tools/native/cxx",
            OPERATION_COMPILER_QUERY,
        ),
        completion: None,
    })?;
    ledger.complete("evt-cxx-query-1", native_success())?;
    Ok(ledger)
}

fn measured(value: u64) -> MeasuredU64 {
    MeasuredU64 {
        value: Some(value),
        unknown_reason: None,
    }
}

fn provide_measured(
    session: &mut ObservationSession,
    ledger: &CompilerLedger,
    native: &NativeLedger,
) -> Result<(), Box<dyn Error>> {
    let compiler = session.compiler_evidence(
        Some(ledger.dependency_compilations()),
        Some(ledger.application_compilations()),
        Some(ledger.queries()),
        Some(ledger.failed()),
        Some(ledger.interrupted()),
        "ledger-drained rust compiler events",
    )?;
    session.provide_compiler(compiler)?;
    let observed = session.native_evidence(
        native.measured_native_compilations(),
        Some(native.queries()),
        Some(native.failed()),
        Some(native.interrupted()),
        "ledger-drained native calls",
    )?;
    session.provide_native(observed)?;
    let origin = session.origin_evidence(Some(2), Some(1_048_576), "origin fetch receipts")?;
    session.provide_origin(origin)?;
    let cache = session.cache_evidence(Some(2_097_152), "shared-root restore bytes")?;
    session.provide_cache(cache)?;
    Ok(())
}

fn check_summary_matches_ledgers(report: &ObservationReport) -> Result<(), Box<dyn Error>> {
    if report.schema != 1 {
        return Err(fail("report lost its schema"));
    }
    if report.recipe_id != "internal/lint/dev"
        || report.check_id != "gc1-t19-check"
        || report.execution_context_digest != context_digest()
    {
        return Err(fail("report lost its recipe binding"));
    }
    if report.session_id.is_empty() {
        return Err(fail("report lost its session"));
    }
    if report.child_failed {
        return Err(fail("successful child reported as failed"));
    }
    if report.dependency_compilations != measured(3) {
        return Err(fail("dependency compilations drifted"));
    }
    if report.application_compilations != measured(1) {
        return Err(fail("application compilations drifted"));
    }
    if report.compiler_queries != measured(1) {
        return Err(fail("compiler queries drifted"));
    }
    if report.compiler_failed_calls != measured(2) {
        return Err(fail("failed compiler calls drifted"));
    }
    if report.compiler_interrupted_calls != measured(1) {
        return Err(fail("interrupted compiler calls drifted"));
    }
    if report.native_compilations != measured(1) {
        return Err(fail("native compilations drifted"));
    }
    if report.native_queries != measured(1) {
        return Err(fail("native queries drifted"));
    }
    if report.native_failed_calls != measured(0) {
        return Err(fail("failed native calls drifted"));
    }
    if report.native_interrupted_calls != measured(0) {
        return Err(fail("interrupted native calls drifted"));
    }
    if report.origin_acquisitions != measured(2) {
        return Err(fail("origin acquisitions merged or drifted"));
    }
    if report.origin_bytes != measured(1_048_576) {
        return Err(fail("origin bytes merged with cache bytes"));
    }
    if report.cache_bytes != measured(2_097_152) {
        return Err(fail("cache bytes merged with origin bytes"));
    }
    if !report.gaps.is_empty() {
        return Err(fail("complete observation reported gaps"));
    }
    if report.finished_epoch_ms < report.started_epoch_ms {
        return Err(fail("report finished before it started"));
    }
    if report.observation_duration_ms
        != report
            .finished_epoch_ms
            .saturating_sub(report.started_epoch_ms)
    {
        return Err(fail("observation duration drifted"));
    }
    Ok(())
}

fn check_json_round_trip(report: &ObservationReport) -> Result<(), Box<dyn Error>> {
    let first = report
        .to_canonical_json()
        .map_err(|error| fail(&error.to_string()))?;
    let second = report
        .to_canonical_json()
        .map_err(|error| fail(&error.to_string()))?;
    if first != second {
        return Err(fail("canonical report bytes drifted"));
    }
    let parsed: ObservationReport =
        serde_json::from_slice(&first).map_err(|error| fail(&error.to_string()))?;
    if parsed != *report {
        return Err(fail("report did not survive its canonical bytes"));
    }
    Ok(())
}

#[path = "gc1_t19_a.rs"]
mod gc1_t19_a;
#[path = "gc1_t19_b.rs"]
mod gc1_t19_b;
pub(crate) use gc1_t19_a::*;
