// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

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

fn rust_invocation(
    event_id: &str,
    role: &str,
    operation: &str,
    tool: &str,
    phase: Option<&str>,
) -> CompilerInvocation {
    CompilerInvocation {
        event_id: event_id.to_owned(),
        recipe_id: "internal/lint/dev".to_owned(),
        package_id: "mylib".to_owned(),
        dependency_role: role.to_owned(),
        lane_id: "build".to_owned(),
        tool_kind: tool.to_owned(),
        executable: "/tools/rust/prefix/bin/rustc".to_owned(),
        argv: rust_argv(),
        wrapper_chain: Vec::new(),
        operation_kind: operation.to_owned(),
        doc_phase: phase.map(str::to_owned),
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

fn runtime_ledger() -> Result<CompilerLedger, Box<dyn Error>> {
    let mut ledger = CompilerLedger::new();
    for (id, tool, phase) in [
        ("evt-dep-1", TOOL_KIND_RUSTC, None),
        ("evt-dep-2", TOOL_KIND_RUSTC, None),
        ("evt-doc-1", TOOL_KIND_RUSTDOC, Some(DOC_PHASE_COMPILATION)),
    ] {
        ledger.record(CompilerEvent {
            invocation: rust_invocation(id, ROLE_DEPENDENCY, OPERATION_COMPILATION, tool, phase),
            completion: None,
        })?;
        ledger.complete(id, rust_success())?;
    }
    ledger.record(CompilerEvent {
        invocation: rust_invocation(
            "evt-app-1",
            ROLE_APPLICATION,
            OPERATION_COMPILATION,
            TOOL_KIND_RUSTC,
            None,
        ),
        completion: None,
    })?;
    ledger.complete("evt-app-1", rust_success())?;
    ledger.record(CompilerEvent {
        invocation: rust_invocation(
            "evt-query-1",
            ROLE_APPLICATION,
            OPERATION_COMPILER_QUERY,
            TOOL_KIND_RUSTC,
            None,
        ),
        completion: None,
    })?;
    ledger.complete("evt-query-1", rust_success())?;
    ledger.record(CompilerEvent {
        invocation: rust_invocation(
            "evt-finalize-1",
            ROLE_DEPENDENCY,
            OPERATION_NONCOMPILATION,
            TOOL_KIND_RUSTDOC,
            Some(DOC_PHASE_FINALIZATION),
        ),
        completion: None,
    })?;
    ledger.complete("evt-finalize-1", rust_success())?;
    Ok(ledger)
}

fn native_argv() -> Vec<String> {
    ["cc", "-O2", "-c", "src/native.c", "-o", "out/native.o"]
        .into_iter()
        .map(str::to_owned)
        .collect()
}

pub(crate) fn runtime_native() -> Result<NativeLedger, Box<dyn Error>> {
    let mut ledger = NativeLedger::new();
    ledger.record(NativeEvent {
        invocation: NativeInvocation {
            event_id: "evt-cc-1".to_owned(),
            recipe_id: "internal/lint/dev".to_owned(),
            package_id: "libnative".to_owned(),
            dependency_role: ROLE_DEPENDENCY.to_owned(),
            lane_id: "build".to_owned(),
            tool_kind: TOOL_KIND_CC.to_owned(),
            executable: "/tools/native/cc".to_owned(),
            argv: native_argv(),
            adapter_chain: vec!["/tools/native/cc".to_owned()],
            parent_event_id: None,
            operation_kind: OPERATION_COMPILATION.to_owned(),
            cwd: "/repo".to_owned(),
        },
        completion: None,
    })?;
    ledger.complete(
        "evt-cc-1",
        NativeCompletion {
            exit_code: Some(0),
            signal: None,
            interrupted: false,
        },
    )?;
    Ok(ledger)
}

fn runtime_report() -> Result<ObservationReport, Box<dyn Error>> {
    let ledger = runtime_ledger()?;
    if ledger.dependency_compilations() != 3
        || ledger.application_compilations() != 1
        || ledger.queries() != 1
    {
        return Err(fail("runtime ledger misattributes rustdoc work"));
    }
    let native = runtime_native()?;
    let mut session = ObservationSession::begin(
        "internal/lint/dev",
        "gc1-runtime-check",
        &new_session_id(),
        &digest_b3(b"gc1-runtime-observation-root"),
    )?;
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
    Ok(session.finish(ChildOutcome::Success)?)
}

pub(crate) fn check_observer_attributes_runtime_work() -> Result<(), Box<dyn Error>> {
    let report = runtime_report()?;
    report
        .validate()
        .map_err(|error| fail(&error.to_string()))?;
    if report.dependency_compilations.value != Some(3) {
        return Err(fail("rustdoc compilation missing from dependency count"));
    }
    if report.application_compilations.value != Some(1) {
        return Err(fail("application compilation drifted"));
    }
    if report.compiler_queries.value != Some(1) {
        return Err(fail("rustdoc finalization counted as a compiler query"));
    }
    if report.native_compilations.value != Some(1) {
        return Err(fail("native compilation drifted"));
    }
    if report.origin_acquisitions.value != Some(2) || report.origin_bytes.value != Some(1_048_576) {
        return Err(fail("origin acquisitions merged with cache transfers"));
    }
    if report.cache_bytes.value != Some(2_097_152) {
        return Err(fail("cache bytes merged with origin bytes"));
    }
    if !report.gaps.is_empty() {
        return Err(fail("complete runtime observation reported gaps"));
    }
    if report.child_failed {
        return Err(fail("successful runtime reported as failed"));
    }
    let bytes = report
        .to_canonical_json()
        .map_err(|error| fail(&error.to_string()))?;
    let parsed: ObservationReport =
        serde_json::from_slice(&bytes).map_err(|error| fail(&error.to_string()))?;
    if parsed != report {
        return Err(fail("runtime report lost its canonical round trip"));
    }
    let temp = tempfile::tempdir()?;
    let observation_dir = temp.path().join("velnor-gc1").join("observations");
    fs::create_dir_all(&observation_dir)?;
    let home = temp.path().join("velnor-check-0");
    fs::create_dir(&home)?;
    fs::write(observation_dir.join("check-observation.json"), &bytes)?;
    fs::remove_dir_all(&home)?;
    if home.exists() {
        return Err(fail("private home survived teardown"));
    }
    let retained = fs::read(observation_dir.join("check-observation.json"))?;
    let survived: ObservationReport =
        serde_json::from_slice(&retained).map_err(|error| fail(&error.to_string()))?;
    if survived != report {
        return Err(fail("retained report drifted across private-home teardown"));
    }
    Ok(())
}

pub(crate) fn fixture_frozen() -> Result<FrozenArchive, Box<dyn Error>> {
    let mut bytes = b"gc1-runtime-frozen-candidate-marker:".to_vec();
    for round in 0_u32..64 {
        bytes.extend_from_slice(&round.to_le_bytes());
    }
    Ok(freeze_archive(&bytes)?)
}

pub(crate) fn fixture_proof(frozen: &FrozenArchive) -> Result<FrozenProofV1, Box<dyn Error>> {
    Ok(FrozenProofV1 {
        schema: 1,
        producer_job: fixture_job("writer")?,
        generation: fixture_generation(b"gc1-runtime-coverage")?,
        manifest_digest: digest_b3_typed(b"gc1-runtime-manifest"),
        frozen_archive_digest: frozen.archive_sha256.clone(),
        frozen_archive_bytes: frozen.archive_bytes,
        frozen_at: "2026-10-26T00:00:00Z".to_owned(),
        paired_input_archive_bindings: vec![
            PairedInputBinding {
                input_role: "sources".to_owned(),
                archive_digest: digest_b3_typed(b"gc1-runtime-sources"),
            },
            PairedInputBinding {
                input_role: "tools".to_owned(),
                archive_digest: digest_b3_typed(b"gc1-runtime-tools"),
            },
        ],
        restore_context_digest: digest_b3_typed(b"gc1-runtime-restore-context"),
        restored_state_index_digest: digest_b3_typed(b"gc1-runtime-state-index"),
        restored_projection_index_digest: digest_b3_typed(b"gc1-runtime-projection-index"),
        metadata_roundtrip_passed: true,
        ephemeral_mbx_initial_state: MbxInitialState::Empty,
        ephemeral_mbx_evidence: "ephemeral MBX store observed empty before replay".to_owned(),
        replay_recipe_ids: vec!["check".to_owned(), "clippy".to_owned(), "test".to_owned()],
        replay_command_ledger_digest: digest_b3_typed(b"gc1-runtime-ledger"),
        replay_observation_digest: digest_b3_typed(b"gc1-runtime-observation"),
        eligible_dependency_ids_digest: digest_b3_typed(b"gc1-runtime-eligible"),
        eligible_dependency_count: 3,
        unchanged_dependency_compilations: Some(0),
        covered_origin_acquisitions: Some(2),
        observation_gaps: Vec::new(),
        required_outputs_present: true,
        archive_digest_after_replay: frozen.archive_sha256.clone(),
        completed_coverage_digest: digest_b3_typed(b"gc1-runtime-completed"),
        qualification_outcome: QualificationOutcome::Pass,
    })
}
