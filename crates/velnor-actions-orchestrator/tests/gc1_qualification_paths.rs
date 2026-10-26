// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! T62 private-home release and pressure qualification helpers.
//!
//! Included by `gc1_qualification_cases.rs` through the existing
//! `gc1_qualification` module; the `c18` through `c22` tests below exercise
//! private-home named and native commands, every test/Clippy/documentation
//! family, helper reuse with source-free authority, migration overlap, and
//! other-family pressure through real owned interfaces with local fixtures
//! only.

use std::error::Error;
use std::fs;
use std::os::unix::fs::PermissionsExt;

use velnor_actions_contract::canonical::{Digest, digest_b3, digest_b3_typed};
use velnor_actions_contract::workflow::cache_context::JobBindingV1;
use velnor_actions_contract::workflow::cache_identity::{Gc1Family, GenerationId};
use velnor_actions_contract::{NamedCheckLane, NamedCheckLaneVariant};
use velnor_actions_mise::compiler_observer::{
    CompilerCompletion, CompilerEvent, CompilerInvocation, CompilerLedger, DOC_PHASE_COMPILATION,
    DOC_PHASE_DOCTEST_DESCENDANT, DOC_PHASE_FINALIZATION, DOC_PHASE_PROBE, DOC_PHASES,
    OPERATION_COMPILATION, OPERATION_COMPILER_QUERY, ROLE_APPLICATION, ROLE_DEPENDENCY,
    TOOL_KIND_CLIPPY_DRIVER, TOOL_KIND_RUSTC, TOOL_KIND_RUSTDOC, TOOL_KINDS, classify_tool_kind,
    is_supported_doc_phase, is_supported_tool_kind, suggest_doc_phase,
};
use velnor_actions_mise::native_compiler_observer::{
    NativeCompletion, NativeEvent, NativeInvocation, NativeLedger, TOOL_KIND_CC, TOOL_KIND_CXX,
    is_observed_native_executable, resolve_native_tool_kind, select_native_adapter,
};
use velnor_actions_orchestrator::{
    AdmissionDecision, AdmissionRequest, BLOCKED_UNKNOWN_FOREIGN_EXPOSURE,
    BLOCKED_UNKNOWN_INVENTORY, CheckoutState, EntryKind, EntryTupleV1, FrozenInnerPayload,
    OPERATING_CEILING_BYTES, OUTER_MEMBER_NAME, OuterEnvelope, OuterEnvelopeIdentity,
    PreflightMeasurement, ProjectionEntry, RealMtime, RestoreDecision, authenticate_entry,
    build_inventory, decide, decide_admission, measure_envelope, measured_bytes, sha256_hex,
    unknown_bytes,
};

use super::cache_discovery::{
    DiscoveryDecision, DiscoveryRequest, EntryProvenance, RestoreObservation, SelectableEntry,
    decide_discovery, verify_restore,
};
use super::cache_intent::{
    IntentDraft, IntentObservation, IntentRecheck, derive_intent, intent_authorizes_mutation,
    recheck_intent,
};
use super::gc1_qualification_inputs::{
    LIB_ID, LINUX_TARGET, SERDE_ID, base_coverage, cold_ledger, coverage_variant, eligible_ids,
    native_cold_ledger, require_changed_coverage, require_measured_zero, require_nonzero,
    require_same_coverage, require_valid_digests, require_zero, warm_native_ledger,
};
use super::must_fail;
use crate::gc1_qualification_support::{
    FixtureCacheRef, ObservationClosure, Workload, invalid, require_complete_observation,
    require_known_case_ids, require_nonempty_workload, require_qualification_destructive,
};

pub(crate) fn check_paths_harness(case_id: &str, detail: &str) -> Result<(), Box<dyn Error>> {
    require_known_case_ids(&[case_id.to_owned()])?;
    require_nonempty_workload(&Workload {
        recipe_ids: vec!["check".to_owned(), "clippy".to_owned(), "test".to_owned()],
        eligible_dependency_ids: eligible_ids(),
    })?;
    require_complete_observation(&ObservationClosure {
        complete: true,
        detail: detail.to_owned(),
    })?;
    Ok(())
}

fn paths_job(
    event: &str,
    ref_name: &str,
    run_id: u64,
    run_attempt: u64,
    job_id: u64,
    job_key: &str,
) -> Result<JobBindingV1, Box<dyn Error>> {
    let job = JobBindingV1 {
        schema: 1,
        repository_id: 1,
        repository: "tailrocks/velnor-new".to_owned(),
        run_id,
        run_attempt,
        job_id,
        job_key: job_key.to_owned(),
        event: event.to_owned(),
        ref_name: ref_name.to_owned(),
        workflow_sha: format!("{:040x}", 0xa5a5),
        source_sha: format!("{:040x}", 0x5a5a),
        plan_digest: Digest::parse(&digest_b3(b"gc1-qualification-paths-plan"))?,
    };
    job.validate()?;
    Ok(job)
}

fn paths_generation(
    coverage_seed: &[u8],
    group_seed: &[u8],
) -> Result<GenerationId, Box<dyn Error>> {
    Ok(GenerationId {
        family: Gc1Family::State,
        group_digest: Digest::parse(&digest_b3(group_seed))?,
        compatibility_digest: Digest::parse(&digest_b3(b"gc1-qualification-paths-compat"))?,
        coverage_digest: Digest::parse(&digest_b3(coverage_seed))?,
        repair_revision: 0,
    })
}

fn paths_entry(
    generation: &GenerationId,
    ref_name: &str,
    cache_id: u64,
    provenance: EntryProvenance,
) -> SelectableEntry {
    SelectableEntry {
        cache_id,
        repository_id: 1,
        ref_name: ref_name.to_owned(),
        key: generation.cache_key(),
        version: "v1".to_owned(),
        size_in_bytes: 512,
        created_at: "2026-10-26T00:00:00Z".to_owned(),
        provenance,
    }
}

fn paths_discovery(
    job: JobBindingV1,
    generation: GenerationId,
    allowed_refs: Vec<&str>,
    current_ref: &str,
    entries: Vec<SelectableEntry>,
    inventory_complete: bool,
) -> DiscoveryRequest {
    DiscoveryRequest {
        job,
        generation,
        expected_cache_version: "v1".to_owned(),
        allowed_refs: allowed_refs.into_iter().map(str::to_owned).collect(),
        current_ref: current_ref.to_owned(),
        inventory_digest: digest_b3_typed(b"gc1-qualification-paths-inventory"),
        inventory_complete,
        entries,
        active_coverage_ids: vec!["coverage-a".to_owned()],
    }
}

fn paths_frozen_bytes() -> Vec<u8> {
    let mut bytes = b"gc1-qualification-paths-candidate:".to_vec();
    for round in 0_u32..64 {
        bytes.extend_from_slice(&round.to_le_bytes());
    }
    bytes
}

fn paths_envelope(
    inner_bytes: &[u8],
    measured_outer: u64,
    repack_allowance: u64,
) -> Result<OuterEnvelope, Box<dyn Error>> {
    let inner = FrozenInnerPayload {
        archive_sha256: sha256_hex(inner_bytes),
        archive_bytes: u64::try_from(inner_bytes.len())
            .map_err(|_| invalid("fixture length out of range"))?,
    };
    let preflight = PreflightMeasurement {
        measured_outer_bytes: measured_outer,
        outer_sha256: sha256_hex(b"gc1-qualification-paths-outer"),
    };
    let identity = OuterEnvelopeIdentity {
        tool_identity: "gc1-qualification-paths-packer".to_owned(),
        transport_member: OUTER_MEMBER_NAME.to_owned(),
    };
    let measured = measure_envelope(&inner, &preflight, repack_allowance, &identity, true)
        .map_err(|error| invalid(&error.to_string()))?;
    Ok(OuterEnvelope::Measured(measured))
}

#[expect(
    clippy::too_many_arguments,
    reason = "admission inequality needs every charge"
)]
fn paths_admission(
    job: JobBindingV1,
    generation: GenerationId,
    operating_ceiling: u64,
    inventory_sum: u64,
    pending: u64,
    foreign: u64,
    inner: u64,
    envelope: OuterEnvelope,
    slots: u64,
) -> AdmissionRequest {
    AdmissionRequest {
        job,
        generation,
        active_intent_digest: digest_b3_typed(b"gc1-qualification-paths-intent"),
        inventory_digest: digest_b3_typed(b"gc1-qualification-paths-inventory"),
        operating_ceiling_bytes: operating_ceiling,
        inventory_bytes: measured_bytes(inventory_sum),
        pending_owned_bytes: measured_bytes(pending),
        foreign_inflight_bytes: measured_bytes(foreign),
        candidate_inner_bytes: measured_bytes(inner),
        envelope,
        occupied_generation_slots: slots,
        peak_local_disk_bound: measured_bytes(1_000_000),
        available_local_disk: measured_bytes(100_000_000_000),
        local_disk_reserve: 1_000_000,
        claimed_freed_bytes: 0,
    }
}

/// C18: a named check lane keeps its variant and job binding through the
/// serialized boundary; an unknown variant is rejected and the hosted and
/// scale-set variants stay distinct.
pub(crate) fn check_named_lane_identity() -> Result<(), Box<dyn Error>> {
    let lane = NamedCheckLane {
        variant: Some(NamedCheckLaneVariant::Hosted),
        job_id: "check".to_owned(),
    };
    if lane.job_id.is_empty() {
        return Err(invalid("named lane lost its job binding"));
    }
    let bytes = serde_json::to_vec(&lane).map_err(|error| invalid(&error.to_string()))?;
    let parsed: NamedCheckLane =
        serde_json::from_slice(&bytes).map_err(|error| invalid(&error.to_string()))?;
    if parsed != lane {
        return Err(invalid("named lane lost its round trip"));
    }
    if parsed.variant != Some(NamedCheckLaneVariant::Hosted) {
        return Err(invalid("named lane changed its variant"));
    }
    let unpinned = NamedCheckLane {
        variant: None,
        job_id: "check".to_owned(),
    };
    if unpinned.variant == lane.variant {
        return Err(invalid("unpinned lane merged with the hosted lane"));
    }
    let scaled = NamedCheckLane {
        variant: Some(NamedCheckLaneVariant::ScaleSet),
        job_id: "check".to_owned(),
    };
    if scaled == lane {
        return Err(invalid("scale-set lane merged with the hosted lane"));
    }
    must_fail(
        &serde_json::from_str::<NamedCheckLane>(r#"{"variant":"gpu","job_id":"check"}"#)
            .map(|_| ()),
        "unknown lane variant admitted",
    )?;
    must_fail(
        &serde_json::from_str::<NamedCheckLane>(r#"{"variant":"hosted"}"#).map(|_| ()),
        "lane without a job binding admitted",
    )?;
    Ok(())
}

fn paths_native_invocation(home: &str) -> NativeInvocation {
    NativeInvocation {
        event_id: "evt-cc-home-1".to_owned(),
        recipe_id: "internal/lint/dev".to_owned(),
        package_id: LIB_ID.to_owned(),
        dependency_role: ROLE_DEPENDENCY.to_owned(),
        lane_id: "build".to_owned(),
        tool_kind: TOOL_KIND_CC.to_owned(),
        executable: "/tools/native/cc".to_owned(),
        argv: vec![
            "cc".to_owned(),
            "-O2".to_owned(),
            "-c".to_owned(),
            "src/native.c".to_owned(),
            "-o".to_owned(),
            "out/native.o".to_owned(),
        ],
        adapter_chain: vec!["/tools/native/cc".to_owned()],
        parent_event_id: None,
        operation_kind: OPERATION_COMPILATION.to_owned(),
        cwd: home.to_owned(),
    }
}

/// C18: native commands attribute compilations to the observed adapter; an
/// absolute executable outside the adapter chain cannot resolve to a native
/// tool kind, and its unobserved calls block the warm zero.
pub(crate) fn check_native_command_attribution() -> Result<(), Box<dyn Error>> {
    if !is_observed_native_executable("/tools/native/cc", "/tools/native/cc", "/tools/native/cxx") {
        return Err(invalid("observed adapter executable misclassified"));
    }
    if is_observed_native_executable("/usr/bin/cc", "/tools/native/cc", "/tools/native/cxx") {
        return Err(invalid("absolute system compiler claimed as observed"));
    }
    if resolve_native_tool_kind("/tools/native/cc", "/tools/native/cc", "/tools/native/cxx")?
        != TOOL_KIND_CC
    {
        return Err(invalid("adapter executable lost its tool kind"));
    }
    must_fail(
        &resolve_native_tool_kind("/usr/bin/cc", "/tools/native/cc", "/tools/native/cxx")
            .map(|_| ()),
        "unobserved absolute executable resolved to a tool kind",
    )?;
    must_fail(
        &resolve_native_tool_kind(
            "/tools/native/cc",
            "/tools/native/same",
            "/tools/native/same",
        )
        .map(|_| ()),
        "ambiguous adapter pair admitted",
    )?;
    must_fail(
        &select_native_adapter("rustc", "/tools/native/cc", "/tools/native/cxx").map(|_| ()),
        "unknown native tool kind selected an adapter",
    )?;
    if select_native_adapter(TOOL_KIND_CXX, "/tools/native/cc", "/tools/native/cxx")?
        != "/tools/native/cxx"
    {
        return Err(invalid("C++ adapter selection drifted"));
    }
    let cold = native_cold_ledger()?;
    require_nonzero("cold native compilations", cold.dependency_compilations())?;
    if cold.measured_native_compilations() != Some(1) {
        return Err(invalid("cold native work lost its measured compilation"));
    }
    let mut blind = native_cold_ledger()?;
    blind.record_unobserved_absolute("/usr/bin/cc")?;
    if blind.measured_native_compilations().is_some() {
        return Err(invalid("unobserved absolute calls measured as observed"));
    }
    must_fail(
        &require_measured_zero("blind native work", blind.measured_native_compilations()),
        "unobserved native work admitted as warm zero",
    )?;
    let warm = warm_native_ledger();
    require_measured_zero("warm native work", warm.measured_native_compilations())?;
    Ok(())
}

/// C18: restored bytes land in the exact private home the child commands use;
/// identical home content authenticates unchanged while any byte or mode drift
/// is detected and never scheduled for restore.
pub(crate) fn check_private_home_restore() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let home = temp.path().join("private-home");
    fs::create_dir_all(&home)?;
    let declared = ProjectionEntry {
        path: "cargo-home/bin/tool".to_owned(),
        kind: EntryKind::File,
        mode: 0o755,
        content_sha256: sha256_hex(b"home-tool-bytes"),
        link_target: None,
        mtime: Some(RealMtime {
            seconds: 946_684_800,
            nanoseconds: 0,
        }),
    };
    declared.validate()?;
    if authenticate_entry(&home, &declared)? != CheckoutState::Missing {
        return Err(invalid("empty private home misclassified its state"));
    }
    let full = home.join("cargo-home/bin/tool");
    if let Some(parent) = full.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&full, b"home-tool-bytes")?;
    fs::set_permissions(&full, fs::Permissions::from_mode(0o755))?;
    if authenticate_entry(&home, &declared)? != CheckoutState::Unchanged {
        return Err(invalid("restored home bytes rejected in the exact home"));
    }
    if decide(Some(&declared), Some(&declared), CheckoutState::Unchanged)
        != RestoreDecision::Restore
    {
        return Err(invalid("unchanged home bytes not scheduled for restore"));
    }
    fs::write(&full, b"other bytes")?;
    if authenticate_entry(&home, &declared)? != CheckoutState::Changed {
        return Err(invalid("home content drift missed"));
    }
    if decide(Some(&declared), Some(&declared), CheckoutState::Changed)
        != RestoreDecision::KeepCurrent
    {
        return Err(invalid("changed home bytes scheduled for overwrite"));
    }
    let invocation = paths_native_invocation(
        home.to_str()
            .ok_or_else(|| invalid("private home is not representable"))?,
    );
    invocation
        .validate()
        .map_err(|error| invalid(&error.to_string()))?;
    let mut ledger = NativeLedger::new();
    ledger.record(NativeEvent {
        invocation,
        completion: None,
    })?;
    ledger.complete(
        "evt-cc-home-1",
        NativeCompletion {
            exit_code: Some(0),
            signal: None,
            interrupted: false,
        },
    )?;
    require_nonzero("home native compilations", ledger.dependency_compilations())?;
    Ok(())
}

/// C19: the command inventory covers exactly the rustc, clippy-driver, and
/// rustdoc families; unknown tool kinds never classify to a family adapter.
pub(crate) fn check_tool_kind_coverage() -> Result<(), Box<dyn Error>> {
    if TOOL_KINDS != [TOOL_KIND_RUSTC, TOOL_KIND_CLIPPY_DRIVER, TOOL_KIND_RUSTDOC] {
        return Err(invalid("command family inventory drifted"));
    }
    for kind in TOOL_KINDS {
        if !is_supported_tool_kind(kind) {
            return Err(invalid("listed command family rejected"));
        }
    }
    must_fail(
        &is_supported_tool_kind("turbo")
            .then_some(())
            .ok_or_else(|| invalid("unknown tool kind admitted")),
        "unknown tool kind admitted",
    )?;
    if classify_tool_kind("/tools/rust/prefix/bin/rustc")? != TOOL_KIND_RUSTC {
        return Err(invalid("rustc executable lost its family"));
    }
    if classify_tool_kind("clippy-driver")? != TOOL_KIND_CLIPPY_DRIVER {
        return Err(invalid("clippy-driver executable lost its family"));
    }
    if classify_tool_kind("/tools/rust/prefix/bin/rustdoc")? != TOOL_KIND_RUSTDOC {
        return Err(invalid("rustdoc executable lost its family"));
    }
    must_fail(
        &classify_tool_kind("/usr/bin/turbo").map(|_| ()),
        "unknown executable classified to a command family",
    )?;
    Ok(())
}

/// C19: documentation routing keeps the four supported phases; the suggestion
/// maps doctest descendants, probes, and plain rustdoc compilations, and an
/// unsupported phase never validates.
pub(crate) fn check_doc_phase_routing() -> Result<(), Box<dyn Error>> {
    if DOC_PHASES
        != [
            DOC_PHASE_COMPILATION,
            DOC_PHASE_FINALIZATION,
            DOC_PHASE_PROBE,
            DOC_PHASE_DOCTEST_DESCENDANT,
        ]
    {
        return Err(invalid("documentation phase inventory drifted"));
    }
    for phase in DOC_PHASES {
        if !is_supported_doc_phase(phase) {
            return Err(invalid("listed documentation phase rejected"));
        }
    }
    must_fail(
        &is_supported_doc_phase("draft")
            .then_some(())
            .ok_or_else(|| invalid("unknown doc phase admitted")),
        "unknown documentation phase admitted",
    )?;
    let doctest_argv = vec![
        "rustdoc".to_owned(),
        "--test".to_owned(),
        "src/lib.rs".to_owned(),
    ];
    if suggest_doc_phase(&doctest_argv) != DOC_PHASE_DOCTEST_DESCENDANT {
        return Err(invalid("doctest descendant lost its phase"));
    }
    let probe_argv = vec![
        "rustdoc".to_owned(),
        "--check".to_owned(),
        "src/lib.rs".to_owned(),
    ];
    if suggest_doc_phase(&probe_argv) != DOC_PHASE_PROBE {
        return Err(invalid("documentation probe lost its phase"));
    }
    let plain_argv = vec!["rustdoc".to_owned(), "src/lib.rs".to_owned()];
    if suggest_doc_phase(&plain_argv) != DOC_PHASE_COMPILATION {
        return Err(invalid("plain rustdoc run lost its compilation phase"));
    }
    Ok(())
}

fn paths_family_invocation(
    event_id: &str,
    package_id: &str,
    role: &str,
    tool_kind: &str,
    doc_phase: Option<&str>,
    operation: &str,
) -> CompilerInvocation {
    CompilerInvocation {
        event_id: event_id.to_owned(),
        recipe_id: "internal/lint/dev".to_owned(),
        package_id: package_id.to_owned(),
        dependency_role: role.to_owned(),
        lane_id: "build".to_owned(),
        tool_kind: tool_kind.to_owned(),
        executable: format!("/tools/rust/prefix/bin/{tool_kind}"),
        argv: vec![
            tool_kind.to_owned(),
            "--crate-name".to_owned(),
            "family".to_owned(),
            "--edition=2021".to_owned(),
            "src/lib.rs".to_owned(),
        ],
        wrapper_chain: Vec::new(),
        doc_phase: doc_phase.map(str::to_owned),
        operation_kind: operation.to_owned(),
        cwd: "/repo".to_owned(),
    }
}

fn paths_family_success() -> CompilerCompletion {
    CompilerCompletion {
        exit_code: Some(0),
        signal: None,
        interrupted: false,
    }
}

fn require_full_family_count(dependency_compilations: u64) -> Result<(), Box<dyn Error>> {
    if dependency_compilations != 3 {
        return Err(invalid("command family ledger is incomplete"));
    }
    Ok(())
}

/// C19: rustdoc validates only with a supported documentation phase while
/// rustc and clippy-driver reject any phase; every actual family validates
/// through the same observation boundary.
#[expect(
    clippy::too_many_lines,
    reason = "family validation keeps every tool-kind boundary together"
)]
pub(crate) fn check_family_invocation_validation() -> Result<(), Box<dyn Error>> {
    for phase in DOC_PHASES {
        paths_family_invocation(
            "evt-doc-ok",
            LIB_ID,
            ROLE_DEPENDENCY,
            TOOL_KIND_RUSTDOC,
            Some(phase),
            OPERATION_COMPILATION,
        )
        .validate()
        .map_err(|error| invalid(&error.to_string()))?;
    }
    must_fail(
        &paths_family_invocation(
            "evt-doc-bare",
            LIB_ID,
            ROLE_DEPENDENCY,
            TOOL_KIND_RUSTDOC,
            None,
            OPERATION_COMPILATION,
        )
        .validate(),
        "rustdoc without a documentation phase admitted",
    )?;
    must_fail(
        &paths_family_invocation(
            "evt-doc-bogus",
            LIB_ID,
            ROLE_DEPENDENCY,
            TOOL_KIND_RUSTDOC,
            Some("draft"),
            OPERATION_COMPILATION,
        )
        .validate(),
        "rustdoc with an unknown phase admitted",
    )?;
    must_fail(
        &paths_family_invocation(
            "evt-rustc-phase",
            LIB_ID,
            ROLE_DEPENDENCY,
            TOOL_KIND_RUSTC,
            Some(DOC_PHASE_COMPILATION),
            OPERATION_COMPILATION,
        )
        .validate(),
        "rustc with a documentation phase admitted",
    )?;
    must_fail(
        &paths_family_invocation(
            "evt-clippy-phase",
            LIB_ID,
            ROLE_DEPENDENCY,
            TOOL_KIND_CLIPPY_DRIVER,
            Some(DOC_PHASE_PROBE),
            OPERATION_COMPILATION,
        )
        .validate(),
        "clippy-driver with a documentation phase admitted",
    )?;
    paths_family_invocation(
        "evt-rustc-ok",
        LIB_ID,
        ROLE_DEPENDENCY,
        TOOL_KIND_RUSTC,
        None,
        OPERATION_COMPILATION,
    )
    .validate()
    .map_err(|error| invalid(&error.to_string()))?;
    paths_family_invocation(
        "evt-clippy-ok",
        LIB_ID,
        ROLE_DEPENDENCY,
        TOOL_KIND_CLIPPY_DRIVER,
        None,
        OPERATION_COMPILATION,
    )
    .validate()
    .map_err(|error| invalid(&error.to_string()))?;
    Ok(())
}

/// C19: a complete family ledger attributes rustc, clippy-driver, and rustdoc
/// dependency compilations plus the doctest descendant; a rustc-only ledger
/// never passes as the full family, and a rustdoc version query never counts
/// as documentation compilation.
#[expect(
    clippy::too_many_lines,
    reason = "family ledger keeps full, partial, and query ledgers together"
)]
pub(crate) fn check_family_ledger_completeness() -> Result<(), Box<dyn Error>> {
    let mut full = CompilerLedger::new();
    for (event_id, tool_kind, doc_phase, role) in [
        ("evt-fam-rustc", TOOL_KIND_RUSTC, None, ROLE_DEPENDENCY),
        (
            "evt-fam-clippy",
            TOOL_KIND_CLIPPY_DRIVER,
            None,
            ROLE_DEPENDENCY,
        ),
        (
            "evt-fam-rustdoc",
            TOOL_KIND_RUSTDOC,
            Some(DOC_PHASE_COMPILATION),
            ROLE_DEPENDENCY,
        ),
        (
            "evt-fam-doctest",
            TOOL_KIND_RUSTDOC,
            Some(DOC_PHASE_DOCTEST_DESCENDANT),
            ROLE_APPLICATION,
        ),
    ] {
        full.record(CompilerEvent {
            invocation: paths_family_invocation(
                event_id,
                LIB_ID,
                role,
                tool_kind,
                doc_phase,
                OPERATION_COMPILATION,
            ),
            completion: None,
        })?;
        full.complete(event_id, paths_family_success())?;
    }
    require_nonzero(
        "full family dependency compilations",
        full.dependency_compilations(),
    )?;
    require_nonzero(
        "full family application compilations",
        full.application_compilations(),
    )?;
    require_zero("full family queries", full.queries())?;
    if full.failed() != 0 {
        return Err(invalid("full family recorded failures"));
    }
    if full.dependency_compilations() != 3 {
        return Err(invalid("full family lost a command family"));
    }
    let mut partial = CompilerLedger::new();
    partial.record(CompilerEvent {
        invocation: paths_family_invocation(
            "evt-partial-rustc",
            LIB_ID,
            ROLE_DEPENDENCY,
            TOOL_KIND_RUSTC,
            None,
            OPERATION_COMPILATION,
        ),
        completion: None,
    })?;
    partial.complete("evt-partial-rustc", paths_family_success())?;
    if partial.dependency_compilations() == full.dependency_compilations() {
        return Err(invalid("rustc-only ledger passed as the full family"));
    }
    require_full_family_count(full.dependency_compilations())?;
    if partial.dependency_compilations() != 1 {
        return Err(invalid("rustc-only ledger lost its single compilation"));
    }
    must_fail(
        &require_full_family_count(partial.dependency_compilations()),
        "partial family manufactured full coverage",
    )?;
    let mut query = CompilerLedger::new();
    query.record(CompilerEvent {
        invocation: paths_family_invocation(
            "evt-doc-query",
            LIB_ID,
            ROLE_DEPENDENCY,
            TOOL_KIND_RUSTDOC,
            Some(DOC_PHASE_PROBE),
            OPERATION_COMPILER_QUERY,
        ),
        completion: None,
    })?;
    query.complete("evt-doc-query", paths_family_success())?;
    require_zero(
        "rustdoc query compilations",
        query.dependency_compilations(),
    )?;
    must_fail(
        &require_nonzero(
            "rustdoc query cold control",
            query.dependency_compilations(),
        ),
        "rustdoc version query satisfied the documentation control",
    )?;
    Ok(())
}

/// C20: a changed helper rebuilds because its full identity digest moves,
/// while an application-only change keeps coverage and reuses the unchanged
/// eligible dependencies.
pub(crate) fn check_changed_helper_rebuilds() -> Result<(), Box<dyn Error>> {
    let base = base_coverage()?;
    require_valid_digests(&base)?;
    let helper_bump = coverage_variant("0.1.0", "0.3.0", None, LINUX_TARGET, None, 'f')?;
    require_valid_digests(&helper_bump)?;
    require_changed_coverage(&base, &helper_bump)?;
    if base.coverage_digest == helper_bump.coverage_digest {
        return Err(invalid("changed helper kept the old coverage digest"));
    }
    if base.workspace_digest == helper_bump.workspace_digest {
        return Err(invalid("changed helper kept the old workspace digest"));
    }
    let app_only = coverage_variant("0.1.1", "0.2.0", None, LINUX_TARGET, None, 'f')?;
    require_same_coverage(&base, &app_only)?;
    for member in eligible_ids() {
        if member != LIB_ID && member != SERDE_ID {
            return Err(invalid("eligible dependency set drifted"));
        }
    }
    if !eligible_ids().contains(&LIB_ID.to_owned()) {
        return Err(invalid("changed helper left the eligible set"));
    }
    Ok(())
}

/// C20: identical inputs reproduce identical coverage, so unchanged eligible
/// dependencies are reused instead of recompiled.
pub(crate) fn check_unchanged_inputs_reuse_eligible() -> Result<(), Box<dyn Error>> {
    let first = base_coverage()?;
    let second = coverage_variant("0.1.0", "0.2.0", None, LINUX_TARGET, None, 'f')?;
    require_same_coverage(&first, &second)?;
    require_valid_digests(&second)?;
    Ok(())
}

fn paths_intent_draft() -> IntentDraft {
    IntentDraft {
        repository_id: 1,
        default_ref: "refs/heads/main".to_owned(),
        observed_head_sha: format!("{:040x}", 0xa5a5),
        configuration_digest: digest_b3_typed(b"gc1-qualification-paths-config"),
        active_group_ids: vec!["group-a".to_owned()],
        active_coverage_ids: vec!["coverage-a".to_owned()],
        derived_at: "2026-10-26T00:00:00Z".to_owned(),
        derivation_evidence_digest: digest_b3_typed(b"gc1-qualification-paths-evidence"),
    }
}

fn paths_intent_observation() -> IntentObservation {
    IntentObservation {
        observed_head_sha: format!("{:040x}", 0xa5a5),
        configuration_digest: digest_b3_typed(b"gc1-qualification-paths-config"),
        active_group_ids: vec!["group-a".to_owned()],
        active_coverage_ids: vec!["coverage-a".to_owned()],
    }
}

/// C20: only an unchanged current intent authorizes mutation; changed coverage
/// and a missing observation both refuse, so a signing or publication
/// authority never mutates cached state it did not verify.
pub(crate) fn check_authority_needs_unchanged_intent() -> Result<(), Box<dyn Error>> {
    let intent =
        derive_intent(paths_intent_draft()).map_err(|error| invalid(&error.to_string()))?;
    let unchanged = recheck_intent(&intent, Some(&paths_intent_observation()))
        .map_err(|error| invalid(&error.to_string()))?;
    if unchanged != IntentRecheck::CoverageUnchanged {
        return Err(invalid("unchanged intent lost its authorization"));
    }
    if !intent_authorizes_mutation(unchanged) {
        return Err(invalid("unchanged intent refused mutation"));
    }
    let mut changed_observation = paths_intent_observation();
    changed_observation.active_coverage_ids = vec!["coverage-b".to_owned()];
    let changed = recheck_intent(&intent, Some(&changed_observation))
        .map_err(|error| invalid(&error.to_string()))?;
    if changed != IntentRecheck::CoverageChanged {
        return Err(invalid("changed coverage kept its authorization"));
    }
    must_fail(
        &intent_authorizes_mutation(changed)
            .then_some(())
            .ok_or_else(|| invalid("changed coverage authorized mutation")),
        "changed coverage authorized mutation",
    )?;
    let blocked = recheck_intent(&intent, None).map_err(|error| invalid(&error.to_string()))?;
    if blocked != IntentRecheck::MutationBlocked {
        return Err(invalid("missing observation kept its authorization"));
    }
    must_fail(
        &intent_authorizes_mutation(blocked)
            .then_some(())
            .ok_or_else(|| invalid("missing observation authorized mutation")),
        "missing observation authorized mutation",
    )?;
    Ok(())
}

/// C20: a source-free authority job without entries recovers through the cold
/// path with real nonzero work and never claims a warm exact hit.
pub(crate) fn check_authority_recovers_cold_without_state() -> Result<(), Box<dyn Error>> {
    let generation = paths_generation(
        b"gc1-qualification-paths-authority",
        b"gc1-qualification-paths-group",
    )?;
    let job = paths_job("push", "refs/heads/main", 181, 1, 1811, "authority")?;
    let request = paths_discovery(
        job,
        generation,
        vec!["refs/heads/main"],
        "refs/heads/main",
        Vec::new(),
        true,
    );
    let record = decide_discovery(&request).map_err(|error| invalid(&error.to_string()))?;
    if record.decision == DiscoveryDecision::ExactHit {
        return Err(invalid(
            "authority job claimed cached state it never verified",
        ));
    }
    if record.decision != DiscoveryDecision::ColdMissing {
        return Err(invalid("authority job skipped cold recovery"));
    }
    let cold = cold_ledger()?;
    require_nonzero(
        "authority cold dependency compilations",
        cold.dependency_compilations(),
    )?;
    let warm = warm_native_ledger();
    require_measured_zero("authority native work", warm.measured_native_compilations())?;
    must_fail(
        &require_nonzero("authority warm pass", 0),
        "authority job reported a warm zero-work pass",
    )?;
    Ok(())
}

/// C21: old and new namespace entries are both charged during overlap; the
/// combined inventory admits when it fits and blocks when it overflows, while
/// a complete inventory keeps its exact stored sum.
pub(crate) fn check_overlap_charges_old_and_new() -> Result<(), Box<dyn Error>> {
    let bytes = paths_frozen_bytes();
    let inner = u64::try_from(bytes.len()).map_err(|_| invalid("fixture length out of range"))?;
    let allowance = 100_000_000_u64;
    let measured_outer = 500_000_000_u64;
    let bound = measured_outer + allowance;
    let old_bytes = 7_200_000_000_u64;
    let inventory = build_inventory(
        1,
        "2026-10-26T00:00:00Z",
        vec![EntryTupleV1 {
            repository_id: 1,
            cache_id: 2110,
            ref_name: "refs/heads/main".to_owned(),
            key: paths_generation(
                b"gc1-qualification-paths-old",
                b"gc1-qualification-paths-old-group",
            )?
            .cache_key(),
            version: "v1".to_owned(),
            size_in_bytes: old_bytes,
            created_at: "2026-10-25T00:00:00Z".to_owned(),
            last_accessed_at: "2026-10-26T00:00:00Z".to_owned(),
        }],
        1,
    )
    .map_err(|error| invalid(&error.to_string()))?;
    if inventory.sum_stored_bytes.value != Some(old_bytes) {
        return Err(invalid("migration inventory lost its old-family bytes"));
    }
    let generation = paths_generation(
        b"gc1-qualification-paths-new",
        b"gc1-qualification-paths-new-group",
    )?;
    let job = paths_job("push", "refs/heads/main", 211, 1, 2111, "writer")?;
    let fit = decide_admission(&paths_admission(
        job.clone(),
        generation.clone(),
        OPERATING_CEILING_BYTES,
        old_bytes,
        0,
        0,
        inner,
        paths_envelope(&bytes, measured_outer, allowance)?,
        1,
    ))
    .map_err(|error| invalid(&error.to_string()))?;
    if fit.decision != AdmissionDecision::Admitted {
        return Err(invalid("fitting migration overlap blocked admission"));
    }
    let overflow = decide_admission(&paths_admission(
        job,
        generation,
        OPERATING_CEILING_BYTES,
        OPERATING_CEILING_BYTES - bound + 1,
        0,
        0,
        inner,
        paths_envelope(&bytes, measured_outer, allowance)?,
        1,
    ))
    .map_err(|error| invalid(&error.to_string()))?;
    if overflow.decision != AdmissionDecision::Blocked {
        return Err(invalid("overflowing migration overlap admitted"));
    }
    Ok(())
}

/// C21: the old key never exact-hits the new generation, and old and new
/// namespaces keep distinct keys with strict round trips.
pub(crate) fn check_old_key_never_exact_hits() -> Result<(), Box<dyn Error>> {
    let old = paths_generation(
        b"gc1-qualification-paths-old",
        b"gc1-qualification-paths-old-group",
    )?;
    let new = paths_generation(
        b"gc1-qualification-paths-new",
        b"gc1-qualification-paths-new-group",
    )?;
    if old.cache_key() == new.cache_key() {
        return Err(invalid("migration kept a single namespace key"));
    }
    GenerationId::validate_key(&new.cache_key())?;
    if GenerationId::parse_key(&new.cache_key())? != new {
        return Err(invalid("new generation key lost its strict round trip"));
    }
    must_fail(
        &GenerationId::validate_key("bogus-key"),
        "malformed migration key admitted",
    )?;
    let job = paths_job("push", "refs/heads/main", 212, 1, 2121, "reader")?;
    let request = paths_discovery(
        job,
        new.clone(),
        vec!["refs/heads/main"],
        "refs/heads/main",
        vec![paths_entry(
            &old,
            "refs/heads/main",
            2120,
            EntryProvenance::Trusted,
        )],
        true,
    );
    let record = decide_discovery(&request).map_err(|error| invalid(&error.to_string()))?;
    if record.decision == DiscoveryDecision::ExactHit {
        return Err(invalid("old namespace entry exact-hit the new generation"));
    }
    let exact = RestoreObservation {
        intended_key: new.cache_key(),
        matched_key: new.cache_key(),
        restored_compatibility: new.compatibility_digest.clone(),
        expected_compatibility: new.compatibility_digest.clone(),
        restored_coverage: new.coverage_digest.clone(),
        expected_coverage: new.coverage_digest.clone(),
        restored_manifest: digest_b3_typed(b"gc1-qualification-paths-manifest"),
        intended_manifest: digest_b3_typed(b"gc1-qualification-paths-manifest"),
    };
    verify_restore(&exact).map_err(|error| invalid(&error.to_string()))?;
    let mut stale = exact.clone();
    stale.matched_key = old.cache_key();
    must_fail(
        &verify_restore(&stale),
        "old archive imported into trusted new state",
    )?;
    Ok(())
}

/// C21: explicitly owned superseded IDs delete only after the verified new
/// generation restores; unlisted IDs and foreign namespaces stay protected.
pub(crate) fn check_verified_replacement_gates_owned_delete() -> Result<(), Box<dyn Error>> {
    require_qualification_destructive(
        &FixtureCacheRef {
            cache_id: 2130,
            namespace: "velnor-gc1-qualification".to_owned(),
        },
        &[2130],
    )?;
    must_fail(
        &require_qualification_destructive(
            &FixtureCacheRef {
                cache_id: 9999,
                namespace: "velnor-gc1-qualification".to_owned(),
            },
            &[2130],
        ),
        "unowned superseded ID admitted for deletion",
    )?;
    must_fail(
        &require_qualification_destructive(
            &FixtureCacheRef {
                cache_id: 2130,
                namespace: "production".to_owned(),
            },
            &[2130],
        ),
        "foreign namespace admitted for deletion",
    )?;
    Ok(())
}

/// C22: every active family records its own generation key; sources, tools,
/// and state stay distinct so pressure accounting never merges them.
pub(crate) fn check_all_families_recorded() -> Result<(), Box<dyn Error>> {
    if Gc1Family::ALL.len() != 3 {
        return Err(invalid("cache family inventory drifted"));
    }
    if Gc1Family::Sources.as_str() == Gc1Family::Tools.as_str()
        || Gc1Family::Tools.as_str() == Gc1Family::State.as_str()
        || Gc1Family::Sources.as_str() == Gc1Family::State.as_str()
    {
        return Err(invalid("cache families merged their names"));
    }
    let mut keys = std::collections::BTreeSet::new();
    for family in Gc1Family::ALL {
        let generation = GenerationId {
            family,
            group_digest: Digest::parse(&digest_b3(b"gc1-qualification-paths-group"))?,
            compatibility_digest: Digest::parse(&digest_b3(b"gc1-qualification-paths-compat"))?,
            coverage_digest: Digest::parse(&digest_b3(b"gc1-qualification-paths-coverage"))?,
            repair_revision: 0,
        };
        GenerationId::validate_key(&generation.cache_key())?;
        if !keys.insert(generation.cache_key()) {
            return Err(invalid("two families shared one generation key"));
        }
    }
    if keys.len() != 3 {
        return Err(invalid("family pressure lost a contributor"));
    }
    Ok(())
}

/// C22: the repository-wide ceiling counts the full mixed inventory, not just
/// the Rust namespace; an exact fit admits while a one-byte overflow blocks.
#[expect(
    clippy::too_many_lines,
    reason = "mixed-family ceiling keeps inventory, fit, and overflow together"
)]
pub(crate) fn check_repository_ceiling_counts_all_families() -> Result<(), Box<dyn Error>> {
    let bytes = paths_frozen_bytes();
    let inner = u64::try_from(bytes.len()).map_err(|_| invalid("fixture length out of range"))?;
    let allowance = 100_000_000_u64;
    let measured_outer = 500_000_000_u64;
    let bound = measured_outer + allowance;
    let inventory_sum = OPERATING_CEILING_BYTES - bound;
    let inventory = build_inventory(
        1,
        "2026-10-26T00:00:00Z",
        vec![
            EntryTupleV1 {
                repository_id: 1,
                cache_id: 2210,
                ref_name: "refs/heads/main".to_owned(),
                key: paths_generation(
                    b"gc1-qualification-paths-rust",
                    b"gc1-qualification-paths-group",
                )?
                .cache_key(),
                version: "v1".to_owned(),
                size_in_bytes: 6_000_000_000,
                created_at: "2026-10-25T00:00:00Z".to_owned(),
                last_accessed_at: "2026-10-26T00:00:00Z".to_owned(),
            },
            EntryTupleV1 {
                repository_id: 1,
                cache_id: 2211,
                ref_name: "refs/heads/main".to_owned(),
                key: paths_generation(
                    b"gc1-qualification-paths-other",
                    b"gc1-qualification-paths-group",
                )?
                .cache_key(),
                version: "v1".to_owned(),
                size_in_bytes: inventory_sum - 6_000_000_000,
                created_at: "2026-10-25T00:00:00Z".to_owned(),
                last_accessed_at: "2026-10-26T00:00:00Z".to_owned(),
            },
        ],
        1,
    )
    .map_err(|error| invalid(&error.to_string()))?;
    if inventory.sum_stored_bytes.value != Some(inventory_sum) {
        return Err(invalid("mixed-family inventory lost its stored sum"));
    }
    let generation = paths_generation(
        b"gc1-qualification-paths-pressure",
        b"gc1-qualification-paths-group",
    )?;
    let job = paths_job("push", "refs/heads/main", 221, 1, 2211, "writer")?;
    let fit = decide_admission(&paths_admission(
        job.clone(),
        generation.clone(),
        OPERATING_CEILING_BYTES,
        inventory_sum,
        0,
        0,
        inner,
        paths_envelope(&bytes, measured_outer, allowance)?,
        1,
    ))
    .map_err(|error| invalid(&error.to_string()))?;
    if fit.decision != AdmissionDecision::Admitted {
        return Err(invalid("exact mixed-family fit blocked admission"));
    }
    if fit.candidate_upper_bound_bytes.value != Some(bound) {
        return Err(invalid("admission lost its qualified upper bound"));
    }
    let over = decide_admission(&paths_admission(
        job,
        generation,
        OPERATING_CEILING_BYTES,
        inventory_sum + 1,
        0,
        0,
        inner,
        paths_envelope(&bytes, measured_outer, allowance)?,
        1,
    ))
    .map_err(|error| invalid(&error.to_string()))?;
    if over.decision != AdmissionDecision::Blocked {
        return Err(invalid("one-byte mixed-family overflow admitted"));
    }
    Ok(())
}

/// C22: bounded foreign in-flight writes are charged against the same ceiling,
/// and unknown inventory or unknown foreign exposure blocks publication while
/// ordinary checks stay usable.
pub(crate) fn check_unknown_pressure_blocks() -> Result<(), Box<dyn Error>> {
    let bytes = paths_frozen_bytes();
    let inner = u64::try_from(bytes.len()).map_err(|_| invalid("fixture length out of range"))?;
    let allowance = 100_000_000_u64;
    let measured_outer = 500_000_000_u64;
    let bound = measured_outer + allowance;
    let inventory_sum = OPERATING_CEILING_BYTES - bound;
    let generation = paths_generation(
        b"gc1-qualification-paths-pressure",
        b"gc1-qualification-paths-group",
    )?;
    let job = paths_job("push", "refs/heads/main", 222, 1, 2221, "writer")?;
    let charged = decide_admission(&paths_admission(
        job.clone(),
        generation.clone(),
        OPERATING_CEILING_BYTES,
        inventory_sum,
        0,
        1,
        inner,
        paths_envelope(&bytes, measured_outer, allowance)?,
        1,
    ))
    .map_err(|error| invalid(&error.to_string()))?;
    if charged.decision != AdmissionDecision::Blocked {
        return Err(invalid("foreign pressure escaped the ceiling charge"));
    }
    let mut unknown_inventory = paths_admission(
        job.clone(),
        generation.clone(),
        OPERATING_CEILING_BYTES,
        inventory_sum,
        0,
        0,
        inner,
        paths_envelope(&bytes, measured_outer, allowance)?,
        1,
    );
    unknown_inventory.inventory_bytes = unknown_bytes("gc1-qualification-paths-inventory")
        .map_err(|error| invalid(&error.to_string()))?;
    let blocked_inventory =
        decide_admission(&unknown_inventory).map_err(|error| invalid(&error.to_string()))?;
    if blocked_inventory.decision != AdmissionDecision::Blocked {
        return Err(invalid("unknown mixed inventory admitted"));
    }
    if !blocked_inventory
        .blocked_reasons
        .contains(&BLOCKED_UNKNOWN_INVENTORY.to_owned())
    {
        return Err(invalid("unknown mixed inventory missed its reason"));
    }
    let mut unknown_foreign = paths_admission(
        job,
        generation,
        OPERATING_CEILING_BYTES,
        inventory_sum,
        0,
        0,
        inner,
        paths_envelope(&bytes, measured_outer, allowance)?,
        1,
    );
    unknown_foreign.foreign_inflight_bytes = unknown_bytes("gc1-qualification-paths-foreign")
        .map_err(|error| invalid(&error.to_string()))?;
    let blocked_foreign =
        decide_admission(&unknown_foreign).map_err(|error| invalid(&error.to_string()))?;
    if blocked_foreign.decision != AdmissionDecision::Blocked {
        return Err(invalid("unknown foreign exposure admitted"));
    }
    if !blocked_foreign
        .blocked_reasons
        .contains(&BLOCKED_UNKNOWN_FOREIGN_EXPOSURE.to_owned())
    {
        return Err(invalid("unknown foreign exposure missed its reason"));
    }
    Ok(())
}
