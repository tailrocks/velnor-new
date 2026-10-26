// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

macro_rules! must_fail {
    ($result:expr, $case:expr $(,)?) => {
        if $result.is_ok() {
            return Err(fail($case));
        }
    };
}

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
    ShadowState, classify_shadow, decide_discovery, discovery_canonical_bytes, discovery_digest,
    parse_discovery_bytes, primary_prefix, select_exact, select_predecessor, verify_restore,
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

#[path = "gc1_runtime_cases_a.rs"]
mod gc1_runtime_cases_a;
#[path = "gc1_runtime_cases_b.rs"]
mod gc1_runtime_cases_b;
#[path = "gc1_runtime_cases_c.rs"]
mod gc1_runtime_cases_c;
pub(crate) use gc1_runtime_cases_a::*;
pub(crate) use gc1_runtime_cases_b::*;
