// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;

use velnor_actions_contract::{Digest, digest_b3};
use velnor_actions_orchestrator::{
    LaneRoot, SnapshotInputs, build_snapshot_plan, pack_snapshot_archive,
};

use super::{expect_code, fixture_generation, fixture_job, write_file};

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
