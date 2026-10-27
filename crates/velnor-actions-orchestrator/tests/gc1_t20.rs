// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

macro_rules! must_fail {
    ($result:expr, $case:expr $(,)?) => {
        if $result.is_ok() {
            return Err(fail($case));
        }
    };
}

use std::error::Error;
use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

use velnor_actions_contract::canonical::Digest;
use velnor_actions_contract::workflow::cache_context::JobBindingV1;
use velnor_actions_contract::workflow::cache_identity::{Gc1Family, GenerationId};
use velnor_actions_contract::{canonical_json_bytes, digest_b3};
use velnor_actions_orchestrator::{
    EntryKind, LaneRoot, ProjectionEntry, RealMtime, RestoreDecision, SnapshotInputs, SnapshotPlan,
    archived_projection_records, build_snapshot_plan, collect_state_index,
    extract_snapshot_archive, manifest_canonical_bytes, pack_snapshot_archive,
    parse_manifest_bytes, parse_projection_index_bytes, parse_state_index_bytes, sha256_hex,
};

fn fail(message: &str) -> Box<dyn Error> {
    io::Error::other(message).into()
}

fn write_file(path: &Path, bytes: &[u8], mode: u32) -> Result<(), Box<dyn Error>> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, bytes)?;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
    Ok(())
}

fn set_mtime(path: &Path, seconds: i64, nanos: u32) -> Result<(), Box<dyn Error>> {
    let stamp = UNIX_EPOCH + Duration::new(u64::try_from(seconds)?, nanos);
    let file = fs::File::open(path)?;
    file.set_times(fs::FileTimes::new().set_modified(stamp))?;
    Ok(())
}

fn read_mtime(path: &Path) -> Result<(i64, u32), Box<dyn Error>> {
    let elapsed = fs::symlink_metadata(path)?
        .modified()?
        .duration_since(UNIX_EPOCH)?;
    Ok((i64::try_from(elapsed.as_secs())?, elapsed.subsec_nanos()))
}

fn mode_of(path: &Path) -> Result<u32, Box<dyn Error>> {
    Ok(fs::symlink_metadata(path)?.permissions().mode() & 0o7777)
}

fn record_for(root: &Path, rel: &str, kind: EntryKind) -> Result<ProjectionEntry, Box<dyn Error>> {
    let absolute = root.join(rel);
    let metadata = fs::symlink_metadata(&absolute)?;
    let elapsed = metadata.modified()?.duration_since(UNIX_EPOCH)?;
    let (content_sha256, link_target) = match kind {
        EntryKind::File => (sha256_hex(&fs::read(&absolute)?), None),
        EntryKind::Directory => (sha256_hex(&[]), None),
        EntryKind::Symlink => {
            let text = fs::read_link(&absolute)?
                .to_str()
                .ok_or_else(|| fail("non utf8 link"))?
                .to_owned();
            (sha256_hex(text.as_bytes()), Some(text))
        }
    };
    Ok(ProjectionEntry {
        path: rel.to_owned(),
        kind,
        mode: mode_of(&absolute)?,
        content_sha256,
        link_target,
        mtime: Some(RealMtime {
            seconds: i64::try_from(elapsed.as_secs())?,
            nanoseconds: elapsed.subsec_nanos(),
        }),
    })
}

fn fixture_job() -> Result<JobBindingV1, Box<dyn Error>> {
    Ok(JobBindingV1 {
        schema: 1,
        repository_id: 1,
        repository: "tailrocks/velnor-new".to_owned(),
        run_id: 7,
        run_attempt: 1,
        job_id: 11,
        job_key: "writer".to_owned(),
        event: "push".to_owned(),
        ref_name: "refs/heads/main".to_owned(),
        workflow_sha: format!("{:040x}", 0xa5a5),
        source_sha: format!("{:040x}", 0x5a5a),
        plan_digest: Digest::parse(&digest_b3(b"gc1-t20-plan"))?,
    })
}

fn fixture_generation() -> Result<GenerationId, Box<dyn Error>> {
    Ok(GenerationId {
        family: Gc1Family::State,
        group_digest: Digest::parse(&digest_b3(b"gc1-t20-group"))?,
        compatibility_digest: Digest::parse(&digest_b3(b"gc1-t20-compat"))?,
        coverage_digest: Digest::parse(&digest_b3(b"gc1-t20-coverage"))?,
        repair_revision: 0,
    })
}

struct FullFixture {
    temp: tempfile::TempDir,
    checkout: PathBuf,
    staged: PathBuf,
    target: PathBuf,
    excluded: PathBuf,
    app_bytes: Vec<u8>,
    out_bytes: Vec<u8>,
    rustdoc_bytes: Vec<u8>,
    lib_bytes: Vec<u8>,
    changed_bytes: Vec<u8>,
    missing_bytes: Vec<u8>,
    app_mtime: (i64, u32),
    lib_mtime: (i64, u32),
    debug_mtime: (i64, u32),
    inputs: SnapshotInputs,
}

fn fixture_inputs(
    checkout: &Path,
    staged: &Path,
    target: &Path,
    build: &Path,
    excluded: &Path,
    declared: Vec<ProjectionEntry>,
    staged_records: Vec<ProjectionEntry>,
) -> Result<SnapshotInputs, Box<dyn Error>> {
    Ok(SnapshotInputs {
        generation: fixture_generation()?,
        producer_job: fixture_job()?,
        source_binding_digest: Digest::parse(&digest_b3(b"gc1-t20-source"))?,
        claimed_completed_recipe_ids: vec!["build/test".to_owned(), "clippy/lint".to_owned()],
        requested_coverage_digest: Digest::parse(&digest_b3(b"gc1-t20-requested"))?,
        compatibility_digest: Digest::parse(&digest_b3(b"gc1-t20-compat"))?,
        paired_source_generation: None,
        paired_tool_generation: None,
        paired_source_archive_digest: None,
        paired_tool_archive_digest: None,
        lanes: vec![
            LaneRoot {
                lane_id: "build".to_owned(),
                root: target.to_owned(),
            },
            LaneRoot {
                lane_id: "test".to_owned(),
                root: build.to_owned(),
            },
        ],
        excluded_roots: vec![excluded.to_owned()],
        excluded_path_policy: "disposable-store-v1".to_owned(),
        projection_staged: staged_records,
        projection_declared: declared,
        checkout_root: checkout.to_owned(),
        staged_projection_root: staged.to_owned(),
        required_output_paths: vec![
            "projection/src/lib.rs".to_owned(),
            "state/build/debug/app".to_owned(),
        ],
    })
}

struct FixtureDirs {
    checkout: PathBuf,
    staged: PathBuf,
    target: PathBuf,
    build: PathBuf,
    excluded: PathBuf,
}

struct FixtureBlobs {
    app: Vec<u8>,
    fingerprint: Vec<u8>,
    rustdoc: Vec<u8>,
    out: Vec<u8>,
    lib: Vec<u8>,
    changed: Vec<u8>,
    old: Vec<u8>,
    missing: Vec<u8>,
}

fn fixture_dirs(temp: &Path) -> FixtureDirs {
    let target = temp.join("owned-state-group-target");
    FixtureDirs {
        checkout: temp.join("repo"),
        staged: temp.join("staged"),
        target: target.clone(),
        build: temp.join("owned-state-group-build"),
        excluded: target.join("disposable-store"),
    }
}

fn write_fixture_trees(dirs: &FixtureDirs, blobs: &FixtureBlobs) -> Result<(), Box<dyn Error>> {
    write_file(&dirs.checkout.join("src/lib.rs"), &blobs.lib, 0o644)?;
    write_file(&dirs.checkout.join("src/changed.rs"), &blobs.changed, 0o644)?;
    write_file(&dirs.staged.join("src/lib.rs"), &blobs.lib, 0o644)?;
    write_file(&dirs.staged.join("src/changed.rs"), &blobs.old, 0o644)?;
    write_file(&dirs.staged.join("src/missing.rs"), &blobs.missing, 0o644)?;
    write_file(&dirs.target.join("debug/app"), &blobs.app, 0o755)?;
    write_file(&dirs.target.join("debug/app.d"), &blobs.fingerprint, 0o644)?;
    write_file(
        &dirs.target.join("debug/.fingerprint/x"),
        &blobs.fingerprint,
        0o644,
    )?;
    write_file(
        &dirs.target.join(".mbx-rustdoc-parts/doc.json"),
        &blobs.rustdoc,
        0o644,
    )?;
    write_file(&dirs.excluded.join("scratch.bin"), b"disposable", 0o644)?;
    write_file(&dirs.build.join("x86_64/out.bin"), &blobs.out, 0o644)?;
    #[cfg(unix)]
    std::os::unix::fs::symlink("app", dirs.target.join("debug/latest"))?;
    Ok(())
}

fn apply_fixture_mtimes(
    dirs: &FixtureDirs,
    app_mtime: (i64, u32),
    lib_mtime: (i64, u32),
    debug_mtime: (i64, u32),
) -> Result<(), Box<dyn Error>> {
    set_mtime(&dirs.target.join("debug/app"), app_mtime.0, app_mtime.1)?;
    set_mtime(&dirs.staged.join("src/lib.rs"), lib_mtime.0, lib_mtime.1)?;
    set_mtime(&dirs.staged.join("src/missing.rs"), 946_684_950, 10)?;
    set_mtime(&dirs.target.join("debug"), debug_mtime.0, debug_mtime.1)?;
    set_mtime(&dirs.target.join("debug/.fingerprint"), 946_685_001, 5)?;
    Ok(())
}

fn fixture_projection_vectors(
    dirs: &FixtureDirs,
) -> Result<(Vec<ProjectionEntry>, Vec<ProjectionEntry>), Box<dyn Error>> {
    let mut declared = vec![
        record_for(&dirs.staged, "src/lib.rs", EntryKind::File)?,
        record_for(&dirs.checkout, "src/changed.rs", EntryKind::File)?,
        record_for(&dirs.staged, "src/missing.rs", EntryKind::File)?,
    ];
    declared.sort_by(|left, right| left.path.cmp(&right.path));
    let mut staged_records = vec![
        record_for(&dirs.staged, "src/lib.rs", EntryKind::File)?,
        record_for(&dirs.staged, "src/changed.rs", EntryKind::File)?,
        record_for(&dirs.staged, "src/missing.rs", EntryKind::File)?,
    ];
    staged_records.sort_by(|left, right| left.path.cmp(&right.path));
    Ok((declared, staged_records))
}

fn build_full_fixture() -> Result<FullFixture, Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let dirs = fixture_dirs(temp.path());
    let blobs = FixtureBlobs {
        app: b"gc1-t20-elf-app".to_vec(),
        fingerprint: b"gc1-t20-fingerprint".to_vec(),
        rustdoc: b"gc1-t20-rustdoc-parts".to_vec(),
        out: b"gc1-t20-test-out".to_vec(),
        lib: b"gc1-t20-lib".to_vec(),
        changed: b"gc1-t20-changed-new".to_vec(),
        old: b"gc1-t20-changed-old".to_vec(),
        missing: b"gc1-t20-missing".to_vec(),
    };
    write_fixture_trees(&dirs, &blobs)?;
    let app_mtime = (946_684_800_i64, 123_456_789_u32);
    let lib_mtime = (946_684_900_i64, 111_222_333_u32);
    let debug_mtime = (946_685_000_i64, 999_000_111_u32);
    apply_fixture_mtimes(&dirs, app_mtime, lib_mtime, debug_mtime)?;
    let (declared, staged_records) = fixture_projection_vectors(&dirs)?;
    let inputs = fixture_inputs(
        &dirs.checkout,
        &dirs.staged,
        &dirs.target,
        &dirs.build,
        &dirs.excluded,
        declared,
        staged_records,
    )?;
    Ok(FullFixture {
        temp,
        checkout: dirs.checkout,
        staged: dirs.staged,
        target: dirs.target,
        excluded: dirs.excluded,
        app_bytes: blobs.app,
        out_bytes: blobs.out,
        rustdoc_bytes: blobs.rustdoc,
        lib_bytes: blobs.lib,
        changed_bytes: blobs.changed,
        missing_bytes: blobs.missing,
        app_mtime,
        lib_mtime,
        debug_mtime,
        inputs,
    })
}

fn planned<'a>(
    plan: &'a SnapshotPlan,
    path: &str,
) -> Result<&'a velnor_actions_orchestrator::PlannedProjectionEntry, Box<dyn Error>> {
    plan.projection
        .iter()
        .find(|entry| entry.record.path == path)
        .ok_or_else(|| fail("planned projection entry missing"))
}

fn check_plan_contents(plan: &SnapshotPlan) -> Result<(), Box<dyn Error>> {
    if plan
        .state_index
        .iter()
        .any(|entry| entry.path.contains("disposable-store"))
    {
        return Err(fail("disposable root leaked into state index"));
    }
    if !plan
        .state_index
        .iter()
        .any(|entry| entry.path == "state/build/.mbx-rustdoc-parts/doc.json")
    {
        return Err(fail("required rustdoc part missing from state index"));
    }
    let lib = planned(plan, "src/lib.rs")?;
    if lib.decision != RestoreDecision::Restore || !lib.staged {
        return Err(fail("unchanged projection must restore staged bytes"));
    }
    let changed = planned(plan, "src/changed.rs")?;
    if changed.decision != RestoreDecision::Discard || changed.staged {
        return Err(fail("mismatched cached projection must stay current"));
    }
    let missing = planned(plan, "src/missing.rs")?;
    if missing.decision != RestoreDecision::KeepCurrent || !missing.staged {
        return Err(fail("missing checkout file must fall back to staged bytes"));
    }
    let manifest_bytes = manifest_canonical_bytes(&plan.manifest)?;
    let manifest_round_trip = parse_manifest_bytes(&manifest_bytes)?;
    if manifest_round_trip != plan.manifest {
        return Err(fail("manifest strict round trip drifted"));
    }
    Ok(())
}

fn check_extracted_indexes(
    plan: &SnapshotPlan,
    extracted: &velnor_actions_orchestrator::ExtractedSnapshot,
) -> Result<(), Box<dyn Error>> {
    if extracted.manifest != plan.manifest {
        return Err(fail("extracted manifest drifted"));
    }
    if extracted.state_index != plan.state_index {
        return Err(fail("extracted state index drifted"));
    }
    if extracted.required_outputs != plan.required_outputs {
        return Err(fail("extracted required outputs drifted"));
    }
    let authoritative = plan
        .projection
        .iter()
        .map(|entry| entry.record.clone())
        .collect::<Vec<_>>();
    if extracted.projection != authoritative {
        return Err(fail("extracted projection drifted"));
    }
    Ok(())
}

#[path = "gc1_t20_a.rs"]
mod gc1_t20_a;
#[path = "gc1_t20_b.rs"]
mod gc1_t20_b;
pub(crate) use gc1_t20_a::*;
