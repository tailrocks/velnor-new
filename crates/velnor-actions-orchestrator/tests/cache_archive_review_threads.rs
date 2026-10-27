// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use flate2::Compression;
use flate2::write::GzEncoder;
use tar::{Builder, EntryType, Header};
use velnor_actions_contract::canonical::Digest;
use velnor_actions_contract::workflow::cache_context::JobBindingV1;
use velnor_actions_contract::workflow::cache_identity::{Gc1Family, GenerationId};
use velnor_actions_contract::{canonical_json_bytes, digest_b3};
use velnor_actions_orchestrator::{
    EntryKind, LaneRoot, PackedArchive, ProjectionEntry, RealMtime, SnapshotInputs, SnapshotPlan,
    StateEntryV1, archived_projection_records, build_snapshot_plan, extract_snapshot_archive,
    freeze_inner_payload, manifest_canonical_bytes, measure_envelope, pack_snapshot_archive,
    projection_index_digest, run_preflight_packaging, sha256_hex, state_index_digest,
};

fn fail(message: &str) -> Box<dyn Error> {
    io::Error::other(message).into()
}

fn expect_code(error: &dyn Error, code: &str) -> Result<(), Box<dyn Error>> {
    if error.to_string().contains(code) {
        Ok(())
    } else {
        Err(fail(&format!("expected '{code}', got '{error}'")))
    }
}

fn write_file(path: &Path, bytes: &[u8], mode: u32) -> Result<(), Box<dyn Error>> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, bytes)?;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
    Ok(())
}

fn record_for(root: &Path, rel: &str, kind: EntryKind) -> Result<ProjectionEntry, Box<dyn Error>> {
    let absolute = root.join(rel);
    let metadata = fs::symlink_metadata(&absolute)?;
    let elapsed = metadata.modified()?.duration_since(std::time::UNIX_EPOCH)?;
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
        mode: metadata.permissions().mode() & 0o7777,
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
        job_key: "review-threads".to_owned(),
        event: "push".to_owned(),
        ref_name: "refs/heads/main".to_owned(),
        workflow_sha: format!("{:040x}", 0xa5a5),
        source_sha: format!("{:040x}", 0x5a5a),
        plan_digest: Digest::parse(&digest_b3(b"review-threads-plan"))?,
    })
}

fn fixture_generation() -> Result<GenerationId, Box<dyn Error>> {
    Ok(GenerationId {
        family: Gc1Family::State,
        group_digest: Digest::parse(&digest_b3(b"review-threads-group"))?,
        compatibility_digest: Digest::parse(&digest_b3(b"review-threads-compat"))?,
        coverage_digest: Digest::parse(&digest_b3(b"review-threads-coverage"))?,
        repair_revision: 0,
    })
}

struct TinyFixture {
    temp: tempfile::TempDir,
    staged: PathBuf,
    plan: SnapshotPlan,
}

fn build_tiny_fixture(out_name: &str, lane_mode: u32) -> Result<TinyFixture, Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let checkout = temp.path().join("repo");
    let staged = temp.path().join("staged");
    let lane = temp.path().join("lane");
    let excluded = lane.join("disposable-store");
    fs::create_dir_all(&excluded)?;
    write_file(&lane.join("out.bin"), b"tiny-out", lane_mode)?;
    write_file(&checkout.join("sibling.txt"), b"sibling", 0o644)?;
    write_file(&staged.join("sibling.txt"), b"sibling", 0o644)?;
    #[cfg(unix)]
    std::os::unix::fs::symlink("sibling.txt", checkout.join("link"))?;
    #[cfg(unix)]
    std::os::unix::fs::symlink("sibling.txt", staged.join("link"))?;
    let declared = vec![
        record_for(&staged, "link", EntryKind::Symlink)?,
        record_for(&staged, "sibling.txt", EntryKind::File)?,
    ];
    let staged_records = vec![
        record_for(&staged, "link", EntryKind::Symlink)?,
        record_for(&staged, "sibling.txt", EntryKind::File)?,
    ];
    let inputs = SnapshotInputs {
        generation: fixture_generation()?,
        producer_job: fixture_job()?,
        source_binding_digest: Digest::parse(&digest_b3(b"review-threads-source"))?,
        claimed_completed_recipe_ids: vec!["build/test".to_owned()],
        requested_coverage_digest: Digest::parse(&digest_b3(b"review-threads-requested"))?,
        compatibility_digest: Digest::parse(&digest_b3(b"review-threads-compat2"))?,
        paired_source_generation: None,
        paired_tool_generation: None,
        paired_source_archive_digest: None,
        paired_tool_archive_digest: None,
        lanes: vec![LaneRoot {
            lane_id: "build".to_owned(),
            root: lane.clone(),
        }],
        excluded_roots: vec![excluded],
        excluded_path_policy: "disposable-store-v1".to_owned(),
        projection_staged: staged_records,
        projection_declared: declared,
        checkout_root: checkout.clone(),
        staged_projection_root: staged.clone(),
        required_output_paths: vec![
            "projection/sibling.txt".to_owned(),
            "state/build/out.bin".to_owned(),
        ],
    };
    let plan = build_snapshot_plan(&inputs)?;
    let archive = temp.path().join(out_name);
    pack_snapshot_archive(&velnor_actions_orchestrator::PackRequest {
        plan: &plan,
        checkout_root: &checkout,
        staged_projection_root: &staged,
        out_path: &archive,
    })?;
    Ok(TinyFixture { temp, staged, plan })
}

fn member_header(mode: u32, kind: EntryType, size: u64) -> Header {
    let mut header = Header::new_gnu();
    header.set_entry_type(kind);
    header.set_size(size);
    header.set_mode(mode);
    header.set_uid(0);
    header.set_gid(0);
    header.set_mtime(0);
    header
}

fn gzip_bytes(
    build: impl FnOnce(&mut Builder<Vec<u8>>) -> Result<(), Box<dyn Error>>,
) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut builder = Builder::new(Vec::new());
    build(&mut builder)?;
    let encoder_bytes = builder.into_inner()?;
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    use std::io::Write;
    encoder.write_all(&encoder_bytes)?;
    Ok(encoder.finish()?)
}

fn repack_plan(
    fixture: &TinyFixture,
    state: &[StateEntryV1],
    projection: &[ProjectionEntry],
    out_name: &str,
) -> Result<PathBuf, Box<dyn Error>> {
    let state_bytes = canonical_json_bytes(state).map_err(|error| fail(&error.to_string()))?;
    let archived = archived_projection_records(projection)?;
    let projection_bytes =
        canonical_json_bytes(&archived).map_err(|error| fail(&error.to_string()))?;
    let required_bytes = canonical_json_bytes(&fixture.plan.required_outputs)
        .map_err(|error| fail(&error.to_string()))?;
    let mut manifest = fixture.plan.manifest.clone();
    manifest.state_index_digest = state_index_digest(state)?;
    manifest.projection_index_digest = projection_index_digest(projection)?;
    let manifest_bytes = manifest_canonical_bytes(&manifest)?;
    let lanes: Vec<(&str, &Path)> = fixture
        .plan
        .lanes
        .iter()
        .map(|lane| (lane.lane_id.as_str(), lane.root.as_path()))
        .collect();
    let lane_root = |member: &str| -> Result<&Path, Box<dyn Error>> {
        let rest = member
            .strip_prefix("state/")
            .ok_or_else(|| fail("state prefix"))?;
        let (lane_id, _) = rest.split_once('/').ok_or_else(|| fail("lane split"))?;
        lanes
            .iter()
            .find(|(id, _)| *id == lane_id)
            .map(|(_, root)| *root)
            .ok_or_else(|| fail("lane missing"))
    };
    let out = fixture.temp.path().join(out_name);
    let file = fs::File::create(&out)?;
    let encoder = GzEncoder::new(file, Compression::default());
    let mut builder = Builder::new(encoder);
    for (name, bytes) in [
        ("manifest.json", manifest_bytes.as_slice()),
        ("state-index.json", state_bytes.as_slice()),
        ("projection-index.json", projection_bytes.as_slice()),
        ("required-output-index.json", required_bytes.as_slice()),
    ] {
        let size = u64::try_from(bytes.len())?;
        let mut header = member_header(0o644, EntryType::Regular, size);
        builder
            .append_data(&mut header, name, bytes)
            .map_err(|error| fail(&error.to_string()))?;
    }
    for entry in state {
        let lane_relative = entry
            .path
            .strip_prefix("state/")
            .and_then(|rest| rest.split_once('/').map(|(_, rel)| rel))
            .ok_or_else(|| fail("state member"))?;
        let absolute = lane_root(&entry.path)?.join(lane_relative);
        match entry.kind.as_str() {
            "symlink" => {
                let mut header = member_header(0o777, EntryType::Symlink, 0);
                builder
                    .append_link(
                        &mut header,
                        entry.path.as_str(),
                        Path::new(entry.link_target.as_deref().unwrap_or("")),
                    )
                    .map_err(|error| fail(&error.to_string()))?;
            }
            "directory" => {
                let mut header = member_header(0o755, EntryType::Directory, 0);
                builder
                    .append_data(&mut header, entry.path.as_str(), std::io::empty())
                    .map_err(|error| fail(&error.to_string()))?;
            }
            _ => {
                let bytes = fs::read(&absolute)?;
                let size = u64::try_from(bytes.len())?;
                let mut header = member_header(entry.mode & 0o777, EntryType::Regular, size);
                builder
                    .append_data(&mut header, entry.path.as_str(), bytes.as_slice())
                    .map_err(|error| fail(&error.to_string()))?;
            }
        }
    }
    for record in projection {
        let member = format!("projection/{}", record.path);
        if let Some(target) = record.link_target.as_deref() {
            let mut header = member_header(0o777, EntryType::Symlink, 0);
            builder
                .append_link(&mut header, member.as_str(), Path::new(target))
                .map_err(|error| fail(&error.to_string()))?;
        } else {
            let root = fixture.staged.as_path();
            let bytes = fs::read(root.join(&record.path))?;
            let size = u64::try_from(bytes.len())?;
            let mut header = member_header(record.mode & 0o777, EntryType::Regular, size);
            builder
                .append_data(&mut header, member.as_str(), bytes.as_slice())
                .map_err(|error| fail(&error.to_string()))?;
        }
    }
    let encoder = builder
        .into_inner()
        .map_err(|error| fail(&error.to_string()))?;
    let mut file = encoder.finish().map_err(|error| fail(&error.to_string()))?;
    use std::io::Write;
    file.flush()?;
    Ok(out)
}

fn projection_records(plan: &SnapshotPlan) -> Vec<ProjectionEntry> {
    plan.projection
        .iter()
        .map(|entry| entry.record.clone())
        .collect()
}

#[test]
fn review_directory_payload_rejected() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let archive = temp.path().join("dir-payload.tar.gz");
    let bytes = gzip_bytes(|builder| {
        let mut header = member_header(0o755, EntryType::Directory, 5);
        builder
            .append_data(&mut header, "state/build/subdir", b"XXXXX".as_slice())
            .map_err(|error| fail(&error.to_string()))?;
        Ok(())
    })?;
    fs::write(&archive, bytes)?;
    let destination = temp.path().join("restore");
    let error = extract_snapshot_archive(&archive, &destination)
        .expect_err("directory payload must be rejected");
    expect_code(&error, "snapshot_archive_nonfile_payload")?;
    Ok(())
}

#[test]
fn review_symlink_payload_rejected() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let archive = temp.path().join("link-payload.tar.gz");
    let bytes = gzip_bytes(|builder| {
        let mut header = member_header(0o777, EntryType::Symlink, 4);
        header
            .set_link_name("app")
            .map_err(|error| fail(&error.to_string()))?;
        builder
            .append_data(&mut header, "state/build/latest", b"YYYY".as_slice())
            .map_err(|error| fail(&error.to_string()))?;
        Ok(())
    })?;
    fs::write(&archive, bytes)?;
    let destination = temp.path().join("restore");
    let error = extract_snapshot_archive(&archive, &destination)
        .expect_err("symlink payload must be rejected");
    expect_code(&error, "snapshot_archive_nonfile_payload")?;
    Ok(())
}

#[test]
fn review_pack_rejects_entry_over_limit() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let lane = temp.path().join("lane");
    let excluded = lane.join("disposable-store");
    fs::create_dir_all(&excluded)?;
    let big = vec![0u8; 256 * 1024 * 1024 + 1];
    write_file(&lane.join("big.bin"), &big, 0o644)?;
    let inputs = SnapshotInputs {
        generation: fixture_generation()?,
        producer_job: fixture_job()?,
        source_binding_digest: Digest::parse(&digest_b3(b"review-threads-source"))?,
        claimed_completed_recipe_ids: vec!["build/test".to_owned()],
        requested_coverage_digest: Digest::parse(&digest_b3(b"review-threads-requested"))?,
        compatibility_digest: Digest::parse(&digest_b3(b"review-threads-compat2"))?,
        paired_source_generation: None,
        paired_tool_generation: None,
        paired_source_archive_digest: None,
        paired_tool_archive_digest: None,
        lanes: vec![LaneRoot {
            lane_id: "build".to_owned(),
            root: lane.clone(),
        }],
        excluded_roots: vec![excluded],
        excluded_path_policy: "disposable-store-v1".to_owned(),
        projection_staged: vec![],
        projection_declared: vec![],
        checkout_root: temp.path().join("repo"),
        staged_projection_root: temp.path().join("staged"),
        required_output_paths: vec!["state/build/big.bin".to_owned()],
    };
    fs::create_dir_all(temp.path().join("repo"))?;
    fs::create_dir_all(temp.path().join("staged"))?;
    let plan = build_snapshot_plan(&inputs)?;
    let out = temp.path().join("payload.tar.gz");
    let error = pack_snapshot_archive(&velnor_actions_orchestrator::PackRequest {
        plan: &plan,
        checkout_root: &inputs.checkout_root,
        staged_projection_root: &inputs.staged_projection_root,
        out_path: &out,
    })
    .expect_err("oversize entry must be rejected at pack time");
    expect_code(&error, "snapshot_archive_entry_size_limit")?;
    Ok(())
}

#[test]
fn review_pack_rejects_special_mode_bits() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let lane = temp.path().join("lane");
    let excluded = lane.join("disposable-store");
    fs::create_dir_all(&excluded)?;
    write_file(&lane.join("setuid.bin"), b"privileged", 0o4755)?;
    let inputs = SnapshotInputs {
        generation: fixture_generation()?,
        producer_job: fixture_job()?,
        source_binding_digest: Digest::parse(&digest_b3(b"review-threads-source"))?,
        claimed_completed_recipe_ids: vec!["build/test".to_owned()],
        requested_coverage_digest: Digest::parse(&digest_b3(b"review-threads-requested"))?,
        compatibility_digest: Digest::parse(&digest_b3(b"review-threads-compat2"))?,
        paired_source_generation: None,
        paired_tool_generation: None,
        paired_source_archive_digest: None,
        paired_tool_archive_digest: None,
        lanes: vec![LaneRoot {
            lane_id: "build".to_owned(),
            root: lane.clone(),
        }],
        excluded_roots: vec![excluded],
        excluded_path_policy: "disposable-store-v1".to_owned(),
        projection_staged: vec![],
        projection_declared: vec![],
        checkout_root: temp.path().join("repo"),
        staged_projection_root: temp.path().join("staged"),
        required_output_paths: vec!["state/build/setuid.bin".to_owned()],
    };
    fs::create_dir_all(temp.path().join("repo"))?;
    fs::create_dir_all(temp.path().join("staged"))?;
    let plan = build_snapshot_plan(&inputs)?;
    assert!(
        plan.state_index
            .iter()
            .any(|entry| entry.mode & !0o777 != 0),
        "fixture must carry special mode bits"
    );
    let out = temp.path().join("payload.tar.gz");
    let error = pack_snapshot_archive(&velnor_actions_orchestrator::PackRequest {
        plan: &plan,
        checkout_root: &inputs.checkout_root,
        staged_projection_root: &inputs.staged_projection_root,
        out_path: &out,
    })
    .expect_err("setuid entry must be rejected at pack time");
    expect_code(&error, "snapshot_mode_special_bits")?;
    Ok(())
}

#[test]
fn review_projection_symlink_hash_drift_rejected() -> Result<(), Box<dyn Error>> {
    let fixture = build_tiny_fixture("payload.tar.gz", 0o644)?;
    let mut projection = projection_records(&fixture.plan);
    let record = projection
        .iter_mut()
        .find(|record| record.kind == EntryKind::Symlink)
        .ok_or_else(|| fail("tiny plan must contain a projection symlink"))?;
    record.content_sha256 = sha256_hex(b"drifted-target-bytes");
    let tampered = repack_plan(
        &fixture,
        &fixture.plan.state_index,
        &projection,
        "drift.tar.gz",
    )?;
    let destination = fixture.temp.path().join("restore-drift");
    let error = extract_snapshot_archive(&tampered, &destination)
        .expect_err("projection symlink hash drift must be rejected");
    expect_code(&error, "snapshot_archive_link_mismatch")?;
    Ok(())
}

#[test]
fn review_state_special_mode_bits_rejected_at_extract() -> Result<(), Box<dyn Error>> {
    let fixture = build_tiny_fixture("payload.tar.gz", 0o644)?;
    let mut state = fixture.plan.state_index.clone();
    let entry = state
        .iter_mut()
        .find(|entry| entry.link_target.is_none())
        .ok_or_else(|| fail("tiny plan must contain a regular state entry"))?;
    entry.mode |= 0o4000;
    let tampered = repack_plan(
        &fixture,
        &state,
        &projection_records(&fixture.plan),
        "setuid.tar.gz",
    )?;
    let destination = fixture.temp.path().join("restore-setuid");
    let error = extract_snapshot_archive(&tampered, &destination)
        .expect_err("special mode bits must be rejected at extract time");
    expect_code(&error, "snapshot_mode_special_bits")?;
    Ok(())
}

#[test]
fn review_stale_preflight_rejected() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let inner_a = temp.path().join("inner-a.tar.gz");
    let inner_b = temp.path().join("inner-b.tar.gz");
    fs::write(&inner_a, b"payload-a")?;
    fs::write(&inner_b, b"payload-b-unrelated")?;
    let frozen_a = freeze_inner_payload(&PackedArchive {
        archive_sha256: sha256_hex(b"payload-a"),
        archive_bytes: 9,
        member_count: 1,
    })?;
    let preflight_b = run_preflight_packaging(&inner_b, &temp.path().join("outer-b.tar.gz"))?;
    let error = measure_envelope(
        &frozen_a,
        &preflight_b,
        0,
        &velnor_actions_orchestrator::OuterEnvelopeIdentity {
            tool_identity: "review-threads-packer".to_owned(),
            transport_member: "payload.tar.gz".to_owned(),
        },
        true,
    )
    .expect_err("stale preflight must not qualify publication");
    expect_code(&error, "envelope_preflight_inner_mismatch")?;
    let preflight_a = run_preflight_packaging(&inner_a, &temp.path().join("outer-a.tar.gz"))?;
    let measured = measure_envelope(
        &frozen_a,
        &preflight_a,
        0,
        &velnor_actions_orchestrator::OuterEnvelopeIdentity {
            tool_identity: "review-threads-packer".to_owned(),
            transport_member: "payload.tar.gz".to_owned(),
        },
        true,
    )?;
    if !velnor_actions_orchestrator::OuterEnvelope::Measured(measured).qualifies_for_publication() {
        return Err(fail("matching preflight must still qualify"));
    }
    Ok(())
}
