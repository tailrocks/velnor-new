// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[expect(
    clippy::duplicate_mod,
    reason = "T54 pins the shared runtime lifecycle alongside its owning task tests"
)]
#[path = "../src/compiler_observation.rs"]
mod compiler_observation;

#[expect(
    clippy::duplicate_mod,
    reason = "T54 pins the shared runtime lifecycle alongside its owning task tests"
)]
#[expect(
    unreachable_pub,
    reason = "T54 exercises the frozen replay record by path include; the owning task keeps the public surface"
)]
#[path = "../src/cache_coverage_replay.rs"]
mod cache_coverage_replay;

#[expect(
    clippy::duplicate_mod,
    reason = "T54 pins the shared runtime lifecycle alongside its owning task tests"
)]
#[expect(
    unreachable_pub,
    reason = "T54 modules are private to cache_keys; the test exercises them by path include"
)]
#[path = "../src/cache_discovery.rs"]
mod cache_discovery;

#[expect(
    clippy::duplicate_mod,
    reason = "T54 pins the shared runtime lifecycle alongside its owning task tests"
)]
#[expect(
    unreachable_pub,
    reason = "T54 modules are private to cache_keys; the test exercises them by path include"
)]
#[path = "../src/cache_intent.rs"]
mod cache_intent;

use std::collections::BTreeMap;
use std::error::Error;
use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use self::cache_coverage_replay::{
    FrozenArchive, FrozenProofV1, MbxInitialState, PairedInputBinding, QualificationOutcome,
    freeze_archive, parse_replay_bytes, replay_canonical_bytes, replay_digest,
    verify_replay_frozen,
};
use self::cache_discovery::{
    DiscoveryDecision, DiscoveryRequest, EntryProvenance, RestoreObservation, SelectableEntry,
    ShadowState, decide_discovery, discovery_canonical_bytes, discovery_digest,
    parse_discovery_bytes, verify_restore,
};
use self::cache_intent::{
    IntentDraft, IntentObservation, derive_intent, intent_authorizes_mutation,
    intent_canonical_bytes, intent_digest, parse_intent_bytes, recheck_intent,
};
use self::compiler_observation::{
    ChildOutcome, ObservationReport, ObservationSession, new_session_id,
};
use velnor_actions_contract::canonical::{Digest, digest_b3, digest_b3_typed};
use velnor_actions_contract::workflow::cache_context::{
    CacheCapability, CacheDriver, CompilerAdapterV1, ExecutionContextV1, JobBindingV1,
    LaneContextV1,
};
use velnor_actions_contract::workflow::cache_identity::{Gc1Family, GenerationId};
use velnor_actions_mise::compiler_observer::{
    CompilerCompletion, CompilerEvent, CompilerInvocation, CompilerLedger, DOC_PHASE_COMPILATION,
    DOC_PHASE_FINALIZATION, OPERATION_COMPILATION, OPERATION_COMPILER_QUERY,
    OPERATION_NONCOMPILATION, ROLE_APPLICATION, ROLE_DEPENDENCY, TOOL_KIND_RUSTC,
    TOOL_KIND_RUSTDOC,
};
use velnor_actions_mise::native_compiler_observer::{
    NativeCompletion, NativeEvent, NativeInvocation, NativeLedger, TOOL_KIND_CC,
};
use velnor_actions_orchestrator::{
    AdmissionDecision, AdmissionRequest, BLOCKED_GENERATION_SLOTS_EXHAUSTED,
    BLOCKED_INSUFFICIENT_HEADROOM, BLOCKED_UNKNOWN_FOREIGN_EXPOSURE, BLOCKED_UNKNOWN_INVENTORY,
    BLOCKED_UNKNOWN_RUNTIME_ENVELOPE, CheckoutState, EntryKind, EntryTupleV1,
    OPERATING_CEILING_BYTES, OuterEnvelope, ProjectionEntry, RealMtime, RestoreDecision,
    SNAPSHOT_ARCHIVE_FORMAT, SNAPSHOT_MTIME_PRECISION, SnapshotManifestV1, UNKNOWN_TOOL_IDENTITY,
    admission_canonical_bytes, authenticate_entry, build_inventory, decide, decide_admission,
    freeze_inner_payload, install_staged_file, inventory_digest, is_git_path,
    manifest_canonical_bytes, measure_envelope, measured_bytes, parse_admission_bytes,
    parse_manifest_bytes, reconcile, restore_directory_times, sha256_hex, sort_deepest_first,
    staged_path, unknown_bytes, unknown_envelope, validate_link_target_in_root, verify_times,
};

fn fail(message: &str) -> Box<dyn Error> {
    io::Error::other(message).into()
}

const ARCHIVED_SECONDS: i64 = 946_684_800;
const ARCHIVED_NANOS: u32 = 123_456_789;

fn fixture_job(key: &str) -> Result<JobBindingV1, Box<dyn Error>> {
    Ok(JobBindingV1 {
        schema: 1,
        repository_id: 1,
        repository: "tailrocks/velnor-new".to_owned(),
        run_id: 7,
        run_attempt: 1,
        job_id: 11,
        job_key: key.to_owned(),
        event: "push".to_owned(),
        ref_name: "refs/heads/main".to_owned(),
        workflow_sha: format!("{:040x}", 0xa5a5),
        source_sha: format!("{:040x}", 0x5a5a),
        plan_digest: Digest::parse(&digest_b3(b"gc1-runtime-plan"))?,
    })
}

fn fixture_generation(coverage_seed: &[u8]) -> Result<GenerationId, Box<dyn Error>> {
    Ok(GenerationId {
        family: Gc1Family::State,
        group_digest: Digest::parse(&digest_b3(b"gc1-runtime-group"))?,
        compatibility_digest: Digest::parse(&digest_b3(b"gc1-runtime-compat"))?,
        coverage_digest: Digest::parse(&digest_b3(coverage_seed))?,
        repair_revision: 0,
    })
}

fn mtime() -> RealMtime {
    RealMtime {
        seconds: ARCHIVED_SECONDS,
        nanoseconds: ARCHIVED_NANOS,
    }
}

fn file_entry(path: &str, bytes: &[u8], mode: u32) -> ProjectionEntry {
    ProjectionEntry {
        path: path.to_owned(),
        kind: EntryKind::File,
        mode,
        content_sha256: sha256_hex(bytes),
        link_target: None,
        mtime: Some(mtime()),
    }
}

fn write_checkout_file(
    root: &Path,
    path: &str,
    bytes: &[u8],
    mode: u32,
) -> Result<PathBuf, Box<dyn Error>> {
    let full = root.join(path);
    if let Some(parent) = full.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&full, bytes)?;
    fs::set_permissions(&full, fs::Permissions::from_mode(mode))?;
    Ok(full)
}

fn read_mtime(path: &Path) -> Result<(i64, u32), Box<dyn Error>> {
    let elapsed = fs::symlink_metadata(path)?
        .modified()?
        .duration_since(std::time::UNIX_EPOCH)?;
    Ok((i64::try_from(elapsed.as_secs())?, elapsed.subsec_nanos()))
}

fn check_fresh_checkout_reuses_unchanged_inputs() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let checkout = temp.path().join("checkout");
    let staged = temp.path().join("staged");
    fs::create_dir_all(&checkout)?;
    fs::create_dir_all(&staged)?;
    let bytes = b"unchanged workspace dependency input\n";
    let declared = file_entry("vendor/dep/src/lib.rs", bytes, 0o644);
    declared.validate()?;
    write_checkout_file(&checkout, "vendor/dep/src/lib.rs", bytes, 0o644)?;
    if authenticate_entry(&checkout, &declared)? != CheckoutState::Unchanged {
        return Err(fail("fresh matching checkout rejected"));
    }
    if !declared.semantic_matches(&file_entry("vendor/dep/src/lib.rs", bytes, 0o644)) {
        return Err(fail("identical records lost semantic equality"));
    }
    let mut retimed = declared.clone();
    retimed.mtime = Some(RealMtime {
        seconds: 1_700_000_000,
        nanoseconds: 1,
    });
    if !declared.semantic_matches(&retimed) {
        return Err(fail("fresh-checkout mtimes leaked into coverage identity"));
    }
    reconcile(&declared, &declared)?;
    if decide(Some(&declared), Some(&declared), CheckoutState::Unchanged)
        != RestoreDecision::Restore
    {
        return Err(fail("unchanged input not scheduled for restore"));
    }
    let staged_file = write_checkout_file(&staged, "vendor/dep/src/lib.rs", bytes, 0o644)?;
    let destination = staged_path(&checkout, "vendor/dep/src/lib.rs")?;
    let outcome = install_staged_file(&staged_file, &destination, &declared)?;
    if !outcome.mtime_restored {
        return Err(fail("restored input lost its archived metadata flag"));
    }
    verify_times(&destination, &mtime())?;
    if read_mtime(&destination)? != (ARCHIVED_SECONDS, ARCHIVED_NANOS) {
        return Err(fail("original nanoseconds not preserved across restore"));
    }
    if staged_file.exists() {
        return Err(fail("staged file not moved into place"));
    }
    Ok(())
}

fn check_changed_and_deleted_inputs() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let checkout = temp.path().join("checkout");
    fs::create_dir_all(&checkout)?;
    let declared = file_entry("build/input.txt", b"archived", 0o644);
    let current = write_checkout_file(&checkout, "build/input.txt", b"changed", 0o644)?;
    let before = read_mtime(&current)?;
    if authenticate_entry(&checkout, &declared)? != CheckoutState::Changed {
        return Err(fail("changed input accepted as unchanged"));
    }
    if decide(Some(&declared), Some(&declared), CheckoutState::Changed)
        != RestoreDecision::KeepCurrent
    {
        return Err(fail("changed input not kept current"));
    }
    if decide(Some(&declared), Some(&declared), CheckoutState::Missing)
        != RestoreDecision::KeepCurrent
    {
        return Err(fail("deleted input resurrected from cache"));
    }
    if fs::read(&current)? != b"changed" || read_mtime(&current)? != before {
        return Err(fail("changed current file disturbed"));
    }
    let fresh = temp.path().join("fresh");
    fs::create_dir_all(&fresh)?;
    if authenticate_entry(&fresh, &declared)? != CheckoutState::Missing {
        return Err(fail("absent input misclassified on fresh checkout"));
    }
    check_git_and_directory_metadata(&checkout, &declared)?;
    Ok(())
}

fn check_git_and_directory_metadata(
    checkout: &Path,
    declared: &ProjectionEntry,
) -> Result<(), Box<dyn Error>> {
    let git_entry = ProjectionEntry {
        path: ".git/objects/pack".to_owned(),
        kind: EntryKind::File,
        mode: 0o644,
        content_sha256: sha256_hex(b"pack"),
        link_target: None,
        mtime: Some(mtime()),
    };
    if !is_git_path(&git_entry.path) {
        return Err(fail("git path not detected"));
    }
    if decide(Some(&git_entry), Some(&git_entry), CheckoutState::Unchanged)
        != RestoreDecision::KeepCurrent
    {
        return Err(fail("checkout git metadata scheduled for restore"));
    }
    if decide(Some(declared), None, CheckoutState::Unchanged) != RestoreDecision::Discard {
        return Err(fail("cached-only path kept"));
    }
    let mut ordered = vec![
        PathBuf::from("a"),
        PathBuf::from("a/b/c"),
        PathBuf::from("a/b"),
    ];
    sort_deepest_first(&mut ordered);
    if ordered
        != vec![
            PathBuf::from("a/b/c"),
            PathBuf::from("a/b"),
            PathBuf::from("a"),
        ]
    {
        return Err(fail("directory order drifted"));
    }
    let child_stamp = RealMtime {
        seconds: ARCHIVED_SECONDS,
        nanoseconds: 222_222_222,
    };
    let parent_stamp = RealMtime {
        seconds: ARCHIVED_SECONDS,
        nanoseconds: 111_111_111,
    };
    let parents = vec![
        (checkout.join("pkg"), parent_stamp),
        (checkout.join("pkg/a"), parent_stamp),
        (checkout.join("pkg/a/b"), child_stamp),
    ];
    for (dir, _) in &parents {
        fs::create_dir_all(dir)?;
    }
    restore_directory_times(&parents)?;
    verify_times(&checkout.join("pkg"), &parent_stamp)?;
    verify_times(&checkout.join("pkg/a"), &parent_stamp)?;
    verify_times(&checkout.join("pkg/a/b"), &child_stamp)?;
    Ok(())
}

fn lane(id: &str, target: &str, build: Option<&str>) -> Result<LaneContextV1, Box<dyn Error>> {
    Ok(LaneContextV1 {
        lane_id: id.to_owned(),
        command_family: "build/test".to_owned(),
        variant_digest: Digest::parse(&digest_b3(b"gc1-runtime-variant"))?,
        target_dir: target.to_owned(),
        build_dir: build.map(str::to_owned),
    })
}

fn fixture_context() -> Result<ExecutionContextV1, Box<dyn Error>> {
    Ok(ExecutionContextV1 {
        schema: 1,
        job: fixture_job("writer")?,
        generation: fixture_generation(b"gc1-runtime-coverage")?,
        requested_coverage_digest: digest_b3_typed(b"gc1-runtime-requested"),
        source_binding_digest: digest_b3_typed(b"gc1-runtime-source"),
        workspace_root: "/repo".to_owned(),
        owned_root: "/owned/velnor-gc1".to_owned(),
        cargo_home: "/owned/cargo-home".to_owned(),
        rustup_home: "/owned/rustup-home".to_owned(),
        tool_root: "/owned/tools".to_owned(),
        transport_root: "/owned/transport".to_owned(),
        observation_root: "/owned/observations".to_owned(),
        lanes: vec![
            lane("build", "state/build", Some("state/build-out"))?,
            lane("test", "state/test", None)?,
        ],
        compiler_adapters: vec![CompilerAdapterV1 {
            stable_path: "/owned/tools/rustc-wrapper".to_owned(),
            executable_identity: "rustc 1.82.0".to_owned(),
        }],
        selected_driver: CacheDriver::Cargo,
        mbx_target_views: false,
        effective_cache_capability: CacheCapability::Write,
        trusted_default_writer: true,
        private_audience_digest: digest_b3_typed(b"gc1-runtime-audience"),
    })
}

fn check_lane_context_uses_separate_build_dirs() -> Result<(), Box<dyn Error>> {
    let context = fixture_context()?;
    context.validate()?;
    let build = &context.lanes[0];
    let test = &context.lanes[1];
    if build.target_dir == test.target_dir {
        return Err(fail("lanes share a writable target directory"));
    }
    if build.build_dir.is_none() {
        return Err(fail("build lane lost its separate build directory"));
    }
    if context.mbx_target_views {
        return Err(fail("snapshot context enables target views"));
    }
    let first = context.context_digest()?;
    let second = fixture_context()?.context_digest()?;
    if first != second {
        return Err(fail(
            "execution context digest drifted across identical builds",
        ));
    }
    let bytes = context.to_canonical_bytes()?;
    if ExecutionContextV1::parse_json(&String::from_utf8(bytes)?)? != context {
        return Err(fail("execution context lost its canonical round trip"));
    }
    Ok(())
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

fn runtime_native() -> Result<NativeLedger, Box<dyn Error>> {
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

fn check_observer_attributes_runtime_work() -> Result<(), Box<dyn Error>> {
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

fn fixture_frozen() -> Result<FrozenArchive, Box<dyn Error>> {
    let mut bytes = b"gc1-runtime-frozen-candidate-marker:".to_vec();
    for round in 0_u32..64 {
        bytes.extend_from_slice(&round.to_le_bytes());
    }
    Ok(freeze_archive(&bytes)?)
}

fn fixture_proof(frozen: &FrozenArchive) -> Result<FrozenProofV1, Box<dyn Error>> {
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

fn check_frozen_snapshot_replay() -> Result<(), Box<dyn Error>> {
    let frozen = fixture_frozen()?;
    let proof = fixture_proof(&frozen)?;
    verify_replay_frozen(&proof, &frozen).map_err(|error| fail(&error.to_string()))?;
    if proof.ephemeral_mbx_initial_state != MbxInitialState::Empty {
        return Err(fail(
            "replay did not start from an empty ephemeral MBX store",
        ));
    }
    if proof.unchanged_dependency_compilations != Some(0) {
        return Err(fail("replay left unchanged dependency work unmeasured"));
    }
    let bytes = replay_canonical_bytes(&proof).map_err(|error| fail(&error.to_string()))?;
    if parse_replay_bytes(&bytes).map_err(|error| fail(&error.to_string()))? != proof {
        return Err(fail("frozen proof lost its strict round trip"));
    }
    if replay_digest(&proof).map_err(|error| fail(&error.to_string()))?
        != replay_digest(&parse_replay_bytes(&bytes).map_err(|error| fail(&error.to_string()))?)
            .map_err(|error| fail(&error.to_string()))?
    {
        return Err(fail("frozen proof digest drifted across round trip"));
    }
    let marker = b"gc1-runtime-frozen-candidate-marker:";
    if bytes.windows(marker.len()).any(|window| window == marker) {
        return Err(fail("proof carried frozen archive bytes"));
    }
    if proof.archive_digest_after_replay != frozen.archive_sha256 {
        return Err(fail("replay mutated the frozen candidate"));
    }
    Ok(())
}

fn fixture_manifest() -> Result<SnapshotManifestV1, Box<dyn Error>> {
    let mut raw = BTreeMap::new();
    raw.insert("projection".to_owned(), 512_u64);
    raw.insert("state".to_owned(), 1_048_576_u64);
    Ok(SnapshotManifestV1 {
        schema: 1,
        generation: fixture_generation(b"gc1-runtime-coverage")?,
        producer_job: fixture_job("writer")?,
        source_binding_digest: digest_b3_typed(b"gc1-runtime-source"),
        claimed_completed_recipe_ids: vec![
            "check".to_owned(),
            "clippy".to_owned(),
            "test".to_owned(),
        ],
        requested_coverage_digest: digest_b3_typed(b"gc1-runtime-requested"),
        compatibility_digest: digest_b3_typed(b"gc1-runtime-compat"),
        paired_source_generation: None,
        paired_tool_generation: None,
        paired_source_archive_digest: None,
        paired_tool_archive_digest: None,
        lane_roots: vec!["state/build".to_owned(), "state/test".to_owned()],
        projection_index_digest: digest_b3_typed(b"gc1-runtime-projection"),
        state_index_digest: digest_b3_typed(b"gc1-runtime-state"),
        required_output_index_digest: digest_b3_typed(b"gc1-runtime-required"),
        archive_format: SNAPSHOT_ARCHIVE_FORMAT.to_owned(),
        mtime_precision: SNAPSHOT_MTIME_PRECISION.to_owned(),
        excluded_path_policy_digest: digest_b3_typed(b"gc1-runtime-excluded"),
        raw_bytes_by_component: raw,
        entry_count: 7,
    })
}

fn check_snapshot_manifest_freezes_claim() -> Result<(), Box<dyn Error>> {
    let manifest = fixture_manifest()?;
    manifest.validate()?;
    if manifest.claimed_completed_recipe_ids.len() != 3 {
        return Err(fail("snapshot claim lost a completed recipe"));
    }
    let bytes = manifest_canonical_bytes(&manifest)?;
    if parse_manifest_bytes(&bytes)? != manifest {
        return Err(fail("snapshot manifest lost its strict round trip"));
    }
    Ok(())
}

fn inventory_entry(cache_id: u64, key: &str, bytes: u64) -> EntryTupleV1 {
    EntryTupleV1 {
        repository_id: 1,
        cache_id,
        ref_name: "refs/heads/main".to_owned(),
        key: key.to_owned(),
        version: "v1".to_owned(),
        size_in_bytes: bytes,
        created_at: "2026-10-25T00:00:00Z".to_owned(),
        last_accessed_at: "2026-10-25T01:00:00Z".to_owned(),
    }
}

fn hosted_request(inventory_sum: u64, bound: u64) -> Result<AdmissionRequest, Box<dyn Error>> {
    let generation = fixture_generation(b"gc1-runtime-coverage")?;
    let entries = vec![
        inventory_entry(3, "velnor-gc1-state-old-baseline", 400),
        inventory_entry(7, &generation.cache_key(), inventory_sum - 400),
    ];
    let inventory = build_inventory(1, "2026-10-25T02:00:00Z", entries, 2)?;
    let digest = inventory_digest(&inventory)?;
    let inner = freeze_inner_payload(&velnor_actions_orchestrator::PackedArchive {
        archive_sha256: sha256_hex(b"gc1-runtime-inner"),
        archive_bytes: 1024,
        member_count: 4,
    })?;
    let measured = measure_envelope(
        &inner,
        &velnor_actions_orchestrator::PreflightMeasurement {
            measured_outer_bytes: bound,
            outer_sha256: sha256_hex(b"gc1-runtime-outer"),
        },
        0,
        &velnor_actions_orchestrator::OuterEnvelopeIdentity {
            tool_identity: "tar-0.4.46+flate2-1.1.10".to_owned(),
            transport_member: "payload.tar.gz".to_owned(),
        },
        true,
    )?;
    Ok(AdmissionRequest {
        job: fixture_job("writer")?,
        generation,
        active_intent_digest: digest_b3_typed(b"gc1-runtime-intent"),
        inventory_digest: digest,
        operating_ceiling_bytes: OPERATING_CEILING_BYTES,
        inventory_bytes: measured_bytes(
            inventory
                .sum_stored_bytes
                .value
                .ok_or_else(|| fail("complete inventory lost its measured sum"))?,
        ),
        pending_owned_bytes: measured_bytes(0),
        foreign_inflight_bytes: measured_bytes(0),
        candidate_inner_bytes: measured_bytes(1024),
        envelope: OuterEnvelope::Measured(measured),
        occupied_generation_slots: 1,
        peak_local_disk_bound: measured_bytes(1_000_000),
        available_local_disk: measured_bytes(100_000_000),
        local_disk_reserve: 1_000_000,
        claimed_freed_bytes: 0,
    })
}

fn fixture_draft(head: &str) -> IntentDraft {
    IntentDraft {
        repository_id: 1,
        default_ref: "refs/heads/main".to_owned(),
        observed_head_sha: head.to_owned(),
        configuration_digest: digest_b3_typed(b"gc1-runtime-config"),
        active_group_ids: vec!["group-a".to_owned()],
        active_coverage_ids: vec!["coverage-a".to_owned()],
        derived_at: "2026-10-26T00:00:00Z".to_owned(),
        derivation_evidence_digest: digest_b3_typed(b"gc1-runtime-evidence"),
    }
}

fn check_all_family_admission_and_intent() -> Result<(), Box<dyn Error>> {
    let request = hosted_request(4_200, 1_000_000)?;
    let admitted = decide_admission(&request)?;
    if admitted.decision != AdmissionDecision::Admitted {
        return Err(fail("complete all-family inventory missed admission"));
    }
    if !admitted.blocked_reasons.is_empty() {
        return Err(fail("admitted candidate kept block reasons"));
    }
    let bytes = admission_canonical_bytes(&admitted)?;
    if parse_admission_bytes(&bytes)? != admitted {
        return Err(fail("admission lost its strict round trip"));
    }
    let intent = derive_intent(fixture_draft(&format!("{:040x}", 0x5a5a)))?;
    let observation = IntentObservation {
        observed_head_sha: format!("{:040x}", 0x5a5a),
        configuration_digest: digest_b3_typed(b"gc1-runtime-config"),
        active_group_ids: vec!["group-a".to_owned()],
        active_coverage_ids: vec!["coverage-a".to_owned()],
    };
    let recheck = recheck_intent(&intent, Some(&observation))?;
    if !intent_authorizes_mutation(recheck) {
        return Err(fail("unchanged active intent blocked mutation"));
    }
    let intent_bytes = intent_canonical_bytes(&intent)?;
    if parse_intent_bytes(&intent_bytes)? != intent {
        return Err(fail("active intent lost its strict round trip"));
    }
    if intent_digest(&intent)? != intent_digest(&parse_intent_bytes(&intent_bytes)?)? {
        return Err(fail("active intent digest drifted across round trip"));
    }
    Ok(())
}

fn check_bounded_repair_key() -> Result<(), Box<dyn Error>> {
    let base = fixture_generation(b"gc1-runtime-coverage")?;
    GenerationId::validate_key(&base.cache_key())?;
    if !base.cache_key().ends_with("-r0") {
        return Err(fail("base generation lost its zero repair revision"));
    }
    let repaired = GenerationId {
        repair_revision: 1,
        ..base.clone()
    };
    GenerationId::validate_key(&repaired.cache_key())?;
    if repaired.cache_key() == base.cache_key() {
        return Err(fail("repair revision did not change the cache key"));
    }
    if !repaired.cache_key().ends_with("-r1") {
        return Err(fail("repair revision drifted from its bounded key"));
    }
    if GenerationId::parse_key(&repaired.cache_key())? != repaired {
        return Err(fail("repair key lost its strict round trip"));
    }
    Ok(())
}

fn selectable(
    cache_id: u64,
    key: &str,
    ref_name: &str,
    provenance: EntryProvenance,
) -> SelectableEntry {
    SelectableEntry {
        cache_id,
        repository_id: 1,
        ref_name: ref_name.to_owned(),
        key: key.to_owned(),
        version: "v1".to_owned(),
        size_in_bytes: 512,
        created_at: "2026-10-25T00:00:00Z".to_owned(),
        provenance,
    }
}

fn check_exact_discovery_first() -> Result<(), Box<dyn Error>> {
    let generation = fixture_generation(b"gc1-runtime-coverage")?;
    let request = DiscoveryRequest {
        job: fixture_job("reader")?,
        generation: generation.clone(),
        expected_cache_version: "v1".to_owned(),
        allowed_refs: vec!["refs/heads/main".to_owned()],
        current_ref: "refs/heads/main".to_owned(),
        inventory_digest: digest_b3_typed(b"gc1-runtime-inventory"),
        inventory_complete: true,
        entries: vec![selectable(
            7,
            &generation.cache_key(),
            "refs/heads/main",
            EntryProvenance::Trusted,
        )],
        active_coverage_ids: vec!["coverage-a".to_owned()],
    };
    let record = decide_discovery(&request)?;
    if record.decision != DiscoveryDecision::ExactHit {
        return Err(fail("exact requested key lost first position"));
    }
    if record.shadow_state != ShadowState::Clear {
        return Err(fail("clean inventory reported a shadow"));
    }
    let candidate = record
        .exact_candidate
        .as_ref()
        .ok_or_else(|| fail("exact hit lost its candidate"))?;
    if candidate.entry.key != generation.cache_key() {
        return Err(fail("exact hit kept the wrong candidate"));
    }
    let bytes = discovery_canonical_bytes(&record)?;
    if parse_discovery_bytes(&bytes)? != record {
        return Err(fail("discovery lost its strict round trip"));
    }
    if discovery_digest(&record)? != discovery_digest(&parse_discovery_bytes(&bytes)?)? {
        return Err(fail("discovery digest drifted across round trip"));
    }
    let observation = RestoreObservation {
        intended_key: generation.cache_key(),
        matched_key: candidate.entry.key.clone(),
        restored_compatibility: generation.compatibility_digest.clone(),
        expected_compatibility: generation.compatibility_digest.clone(),
        restored_coverage: generation.coverage_digest.clone(),
        expected_coverage: generation.coverage_digest.clone(),
        restored_manifest: digest_b3_typed(b"gc1-runtime-manifest"),
        intended_manifest: digest_b3_typed(b"gc1-runtime-manifest"),
    };
    verify_restore(&observation)?;
    Ok(())
}

#[test]
fn gc1_runtime_preserves_contract() -> Result<(), Box<dyn Error>> {
    check_fresh_checkout_reuses_unchanged_inputs()?;
    check_changed_and_deleted_inputs()?;
    check_lane_context_uses_separate_build_dirs()?;
    check_observer_attributes_runtime_work()?;
    check_frozen_snapshot_replay()?;
    check_snapshot_manifest_freezes_claim()?;
    check_all_family_admission_and_intent()?;
    check_bounded_repair_key()?;
    check_exact_discovery_first()?;
    Ok(())
}

macro_rules! must_fail {
    ($result:expr, $case:expr $(,)?) => {
        if $result.is_ok() {
            return Err(fail($case));
        }
    };
}

fn check_rejects_unsafe_source_restoration() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let checkout = temp.path().join("checkout");
    let staged = temp.path().join("staged");
    fs::create_dir_all(&checkout)?;
    fs::create_dir_all(&staged)?;
    let declared = file_entry("src/lib.rs", b"declared", 0o644);
    write_checkout_file(&checkout, "src/lib.rs", b"other bytes", 0o644)?;
    if authenticate_entry(&checkout, &declared)? != CheckoutState::Changed {
        return Err(fail("content mismatch missed"));
    }
    write_checkout_file(&checkout, "src/lib.rs", b"declared", 0o600)?;
    if authenticate_entry(&checkout, &declared)? != CheckoutState::Changed {
        return Err(fail("mode mismatch missed"));
    }
    let mut tampered = declared.clone();
    tampered.content_sha256 = sha256_hex(b"tampered");
    must_fail!(reconcile(&tampered, &declared), "archive mismatch admitted");
    let mut timeless = declared.clone();
    timeless.mtime = None;
    must_fail!(reconcile(&timeless, &declared), "missing mtime admitted");
    let staged_file = write_checkout_file(&staged, "src/lib.rs", b"tampered", 0o644)?;
    let destination = staged_path(&checkout, "src/lib.rs")?;
    must_fail!(
        install_staged_file(&staged_file, &destination, &declared),
        "tampered staged file installed",
    );
    if fs::read(&destination)? != b"declared" {
        return Err(fail("failed install wrote destination"));
    }
    let wrong_stamp = RealMtime {
        seconds: ARCHIVED_SECONDS,
        nanoseconds: 999_999_999,
    };
    must_fail!(
        verify_times(&checkout.join("src/lib.rs"), &wrong_stamp),
        "wrong mtime verified",
    );
    must_fail!(
        validate_link_target_in_root(Path::new("/checkout"), "config/link", "../../escape"),
        "escaping link admitted",
    );
    Ok(())
}

fn check_rejects_shared_context_roots() -> Result<(), Box<dyn Error>> {
    let mut shared = fixture_context()?;
    shared.cargo_home = shared.owned_root.clone();
    must_fail!(shared.validate(), "shared owned roots admitted");
    let mut lanes = fixture_context()?;
    lanes.lanes.clear();
    must_fail!(lanes.validate(), "laneless context admitted");
    let mut duplicate = fixture_context()?;
    let first_lane = duplicate.lanes[0].lane_id.clone();
    duplicate.lanes[1].lane_id = first_lane;
    must_fail!(duplicate.validate(), "duplicate lane admitted");
    let mut views = fixture_context()?;
    views.mbx_target_views = true;
    must_fail!(
        views.validate(),
        "target views admitted in snapshot context"
    );
    let mut adapters = fixture_context()?;
    adapters.compiler_adapters.clear();
    must_fail!(adapters.validate(), "adapterless context admitted");
    Ok(())
}

fn check_rejects_incomplete_observation() -> Result<(), Box<dyn Error>> {
    let mut first = ObservationSession::begin(
        "internal/lint/dev",
        "gc1-runtime-check",
        &new_session_id(),
        &digest_b3(b"gc1-runtime-observation-root"),
    )?;
    let second = ObservationSession::begin(
        "internal/lint/dev",
        "gc1-runtime-check",
        &new_session_id(),
        &digest_b3(b"gc1-runtime-observation-root"),
    )?;
    let foreign = second.compiler_evidence(None, None, None, None, None, "detail")?;
    must_fail!(
        first.provide_compiler(foreign),
        "foreign session evidence admitted",
    );
    let mut native_ledger = runtime_native()?;
    native_ledger.record_unobserved_absolute("/usr/bin/cc")?;
    if native_ledger.measured_native_compilations().is_some() {
        return Err(fail("unobserved native calls measured as zero"));
    }
    let mut partial = ObservationSession::begin(
        "internal/lint/dev",
        "gc1-runtime-check",
        &new_session_id(),
        &digest_b3(b"gc1-runtime-observation-root"),
    )?;
    let compiler = partial.compiler_evidence(None, None, None, None, None, "missing ledger")?;
    partial.provide_compiler(compiler)?;
    must_fail!(
        partial.finish(ChildOutcome::Success),
        "report finished with missing dimensions",
    );
    Ok(())
}

fn check_rejects_unproven_replay() -> Result<(), Box<dyn Error>> {
    let frozen = fixture_frozen()?;
    let proof = fixture_proof(&frozen)?;
    let mut wrong_digest = proof.clone();
    wrong_digest.frozen_archive_digest = "0".repeat(64);
    must_fail!(
        verify_replay_frozen(&wrong_digest, &frozen),
        "foreign frozen digest admitted",
    );
    let mut wrong_bytes = proof.clone();
    wrong_bytes.frozen_archive_bytes += 1;
    must_fail!(
        verify_replay_frozen(&wrong_bytes, &frozen),
        "frozen byte count drift admitted",
    );
    let mut mutated = proof.clone();
    mutated.archive_digest_after_replay = "1".repeat(64);
    must_fail!(
        verify_replay_frozen(&mutated, &frozen),
        "replay-mutated archive admitted",
    );
    let other = freeze_archive(b"gc1-runtime-other-candidate")?;
    must_fail!(
        verify_replay_frozen(&proof, &other),
        "proof verified against another archive",
    );
    let mut compiled = proof.clone();
    compiled.unchanged_dependency_compilations = Some(2);
    must_fail!(
        verify_replay_frozen(&compiled, &frozen),
        "unchanged dependency compilation admitted",
    );
    let mut unknown_compilations = proof.clone();
    unknown_compilations.unchanged_dependency_compilations = None;
    must_fail!(
        verify_replay_frozen(&unknown_compilations, &frozen),
        "unknown compilations admitted as zero",
    );
    let mut gaps = proof.clone();
    gaps.observation_gaps = vec!["native coverage incomplete".to_owned()];
    must_fail!(
        verify_replay_frozen(&gaps, &frozen),
        "open observation gaps admitted",
    );
    let mut timeless = proof.clone();
    timeless.metadata_roundtrip_passed = false;
    must_fail!(
        verify_replay_frozen(&timeless, &frozen),
        "failed metadata round trip admitted",
    );
    let mut missing = proof.clone();
    missing.required_outputs_present = false;
    must_fail!(
        verify_replay_frozen(&missing, &frozen),
        "missing required outputs admitted",
    );
    Ok(())
}

fn check_rejects_blocked_publication() -> Result<(), Box<dyn Error>> {
    let mut unknown_inventory = hosted_request(4_200, 1_000_000)?;
    unknown_inventory.inventory_bytes = unknown_bytes("pagination blocked")?;
    let blocked = decide_admission(&unknown_inventory)?;
    if blocked.decision != AdmissionDecision::Blocked
        || !blocked
            .blocked_reasons
            .contains(&BLOCKED_UNKNOWN_INVENTORY.to_owned())
    {
        return Err(fail("unknown inventory missed its block"));
    }
    let mut foreign = hosted_request(4_200, 1_000_000)?;
    foreign.foreign_inflight_bytes = unknown_bytes("foreign writers unbounded")?;
    let blocked = decide_admission(&foreign)?;
    if !blocked
        .blocked_reasons
        .contains(&BLOCKED_UNKNOWN_FOREIGN_EXPOSURE.to_owned())
    {
        return Err(fail("unknown foreign exposure missed its block"));
    }
    let mut envelope = hosted_request(4_200, 1_000_000)?;
    envelope.envelope = OuterEnvelope::Unknown(unknown_envelope(
        UNKNOWN_TOOL_IDENTITY,
        "pinned tool behavior not yet qualified",
    )?);
    let blocked = decide_admission(&envelope)?;
    if !blocked
        .blocked_reasons
        .contains(&BLOCKED_UNKNOWN_RUNTIME_ENVELOPE.to_owned())
    {
        return Err(fail("unknown runtime envelope missed its block"));
    }
    let mut tight = hosted_request(4_200, OPERATING_CEILING_BYTES)?;
    let blocked = decide_admission(&tight)?;
    if blocked.decision != AdmissionDecision::Blocked
        || !blocked
            .blocked_reasons
            .contains(&BLOCKED_INSUFFICIENT_HEADROOM.to_owned())
    {
        return Err(fail("over-ceiling candidate missed its headroom block"));
    }
    tight.occupied_generation_slots = 2;
    let blocked = decide_admission(&tight)?;
    if !blocked
        .blocked_reasons
        .contains(&BLOCKED_GENERATION_SLOTS_EXHAUSTED.to_owned())
    {
        return Err(fail("full generation slots missed their block"));
    }
    Ok(())
}

fn check_rejects_stale_intent_and_shadow() -> Result<(), Box<dyn Error>> {
    let head = format!("{:040x}", 0x5a5a);
    let intent = derive_intent(fixture_draft(&head))?;
    let moved = recheck_intent(
        &intent,
        Some(&IntentObservation {
            observed_head_sha: format!("{:040x}", 0xa5a5),
            configuration_digest: digest_b3_typed(b"gc1-runtime-config"),
            active_group_ids: vec!["group-a".to_owned()],
            active_coverage_ids: vec!["coverage-b".to_owned()],
        }),
    )?;
    if intent_authorizes_mutation(moved) {
        return Err(fail("stale coverage intent authorized mutation"));
    }
    if intent_authorizes_mutation(recheck_intent(&intent, None)?) {
        return Err(fail("unknown intent authorized mutation"));
    }
    let generation = fixture_generation(b"gc1-runtime-coverage")?;
    let shadowed = DiscoveryRequest {
        job: fixture_job("writer")?,
        generation: generation.clone(),
        expected_cache_version: "v1".to_owned(),
        allowed_refs: vec!["refs/heads/main".to_owned(), "refs/pull/9/merge".to_owned()],
        current_ref: "refs/heads/main".to_owned(),
        inventory_digest: digest_b3_typed(b"gc1-runtime-inventory"),
        inventory_complete: true,
        entries: vec![
            selectable(
                7,
                &generation.cache_key(),
                "refs/heads/main",
                EntryProvenance::Trusted,
            ),
            selectable(
                9,
                &generation.cache_key(),
                "refs/pull/9/merge",
                EntryProvenance::Untrusted,
            ),
        ],
        active_coverage_ids: vec!["coverage-a".to_owned()],
    };
    let refused = decide_discovery(&shadowed)?;
    if refused.decision != DiscoveryDecision::RefusedShadow
        || refused.shadow_state != ShadowState::ConflictingVisibleRef
    {
        return Err(fail("conflicting visible ref escaped the shadow guard"));
    }
    let prefix_mismatch = RestoreObservation {
        intended_key: generation.cache_key(),
        matched_key: format!("{}-suffix", generation.cache_key()),
        restored_compatibility: generation.compatibility_digest.clone(),
        expected_compatibility: generation.compatibility_digest.clone(),
        restored_coverage: generation.coverage_digest.clone(),
        expected_coverage: generation.coverage_digest.clone(),
        restored_manifest: digest_b3_typed(b"gc1-runtime-manifest"),
        intended_manifest: digest_b3_typed(b"gc1-runtime-manifest"),
    };
    must_fail!(
        verify_restore(&prefix_mismatch),
        "prefix-only restore admitted",
    );
    Ok(())
}

#[test]
fn gc1_runtime_rejects_invalid_state() -> Result<(), Box<dyn Error>> {
    check_rejects_unsafe_source_restoration()?;
    check_rejects_shared_context_roots()?;
    check_rejects_incomplete_observation()?;
    check_rejects_unproven_replay()?;
    check_rejects_blocked_publication()?;
    check_rejects_stale_intent_and_shadow()?;
    Ok(())
}
