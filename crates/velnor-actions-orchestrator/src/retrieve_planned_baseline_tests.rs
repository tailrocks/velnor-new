//! Exact plan parents stay bound to service ID, digest, name, and attempt.

use std::io::Write as _;

use super::*;
use crate::cover::revalidate::cover_revalidate_fixtures::{manifest_for, plan_for};
use crate::cover::shard_baseline::AcquiredBaseline;
use crate::merge::BaselineManifest;
use velnor_actions_contract::{BaselineProof, PlanBaseline, canonical_json_bytes, digest_b3};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

fn fixture() -> (Plan, BaselineManifest) {
    let base = "1".repeat(40);
    let mut parent = manifest_for(&base);
    parent.repository_id = digest_b3(b"github.com/o/r");
    let mut plan = plan_for(&parent, Some(&base));
    parent.compatibility_id =
        crate::cover_compat::baseline_compat_for_plan(&plan).expect("compatibility");
    parent.artifact_name =
        velnor_actions_contract::artifact_id_for_baseline(&base, &parent.compatibility_id)
            .expect("artifact name");
    parent.artifact_id = crate::cover_compat::baseline_artifact_numeric_id(&parent.artifact_name);
    let digest = digest_b3(&canonical_json_bytes(&parent).expect("parent bytes"));
    plan.baseline = PlanBaseline::used(
        &base,
        parent.run_id,
        parent.artifact_id,
        &parent.artifact_name,
        &digest,
    )
    .expect("plan baseline");
    plan.obligations[0].baseline_proof = Some(
        BaselineProof::new(
            &base,
            parent.run_id,
            parent.artifact_id,
            &parent.artifact_name,
            &digest,
        )
        .expect("obligation proof"),
    );
    (plan, parent)
}

fn zipped_manifest(parent: &BaselineManifest) -> Vec<u8> {
    let bytes = canonical_json_bytes(parent).expect("canonical parent");
    let mut writer = ZipWriter::new(std::io::Cursor::new(Vec::new()));
    writer
        .start_file(
            "baseline.json",
            SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
        )
        .expect("start ZIP member");
    writer.write_all(&bytes).expect("write manifest");
    writer.finish().expect("finish ZIP").into_inner()
}

fn sha256(bytes: &[u8]) -> String {
    use sha2::{Digest as _, Sha256};
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn attempt_record(parent: &BaselineManifest) -> serde_json::Value {
    serde_json::json!({
        "id": parent.run_id, "run_attempt": parent.run_attempt,
        "head_sha": parent.source_commit, "event": "push", "status": "completed",
        "conclusion": "success", "head_branch": "testmain",
        "path": ".github/workflows/ci.yml", "repository": {"full_name": "o/r"},
        "head_repository": {"full_name": "o/r"}
    })
}

fn resolve_mock(
    plan: &Plan,
    parent: &BaselineManifest,
    metadata: serde_json::Value,
) -> (Option<AcquiredBaseline>, Vec<Vec<String>>) {
    let mut calls = Vec::new();
    let bytes = zipped_manifest(parent);
    let listed = serde_json::json!([{"total_count": 1, "artifacts": [{
        "id": 99,
        "name": parent.artifact_name,
        "expired": false,
        "size_in_bytes": bytes.len(),
        "digest": format!("sha256:{}", sha256(&bytes)),
        "workflow_run": {
            "id": parent.run_id,
            "head_sha": parent.source_commit,
            "head_branch": "testmain"
        }
    }]}]);
    let found = resolve_planned(
        plan,
        "o/r",
        "testmain",
        |args| {
            let args: Vec<String> = args
                .iter()
                .map(|argument| argument.to_string_lossy().into_owned())
                .collect();
            calls.push(args.clone());
            if args[1].ends_with("/artifacts") {
                Ok(listed.to_string())
            } else {
                Ok(metadata.to_string())
            }
        },
        |_, receipt| {
            assert_eq!(receipt.service_id, 99);
            Ok(bytes.clone())
        },
    );
    (found, calls)
}

#[test]
fn selected_parent_survives_newer_same_base_run_and_attempt() {
    let (plan, parent) = fixture();
    let (found, calls) = resolve_mock(&plan, &parent, attempt_record(&parent));
    let found = found.expect("authenticated parent");
    assert_eq!(found.manifest().run_attempt, 1);
    assert!(calls[0].iter().any(|arg| arg == "--paginate"));
    assert!(calls[0].iter().any(|arg| arg == "--slurp"));
    assert_eq!(calls[1], ["api", "repos/o/r/actions/runs/7/attempts/1"]);
}

#[test]
fn wrong_service_attempt_or_provenance_never_authenticates() {
    let (plan, parent) = fixture();
    for (field, wrong) in [
        ("id", serde_json::json!(8)),
        ("run_attempt", serde_json::json!(2)),
        ("head_sha", serde_json::json!("2".repeat(40))),
        ("event", serde_json::json!("pull_request")),
        ("status", serde_json::json!("in_progress")),
        ("conclusion", serde_json::json!("failure")),
        ("head_branch", serde_json::json!("evil")),
        (
            "path",
            serde_json::json!(".github/workflows/ci.yml@release"),
        ),
        ("repository", serde_json::json!({"full_name": "evil/r"})),
        (
            "head_repository",
            serde_json::json!({"full_name": "evil/r"}),
        ),
    ] {
        let mut record = attempt_record(&parent);
        record[field] = wrong;
        assert!(
            resolve_mock(&plan, &parent, record).0.is_none(),
            "reject {field}"
        );
    }
}

#[test]
fn replacement_manifest_attempt_never_matches_selected_plan_digest() {
    let (plan, mut parent) = fixture();
    parent.run_attempt = 2;
    let mut metadata = attempt_record(&parent);
    metadata["run_attempt"] = serde_json::json!(1);
    let (found, calls) = resolve_mock(&plan, &parent, metadata);
    assert!(found.is_none());
    assert_eq!(
        calls.len(),
        1,
        "manifest digest rejects before attempt lookup"
    );
}

#[test]
fn repository_service_casing_preserves_canonical_identity() {
    let (plan, parent) = fixture();
    let mut metadata = attempt_record(&parent);
    metadata["repository"]["full_name"] = serde_json::json!("O/R");
    metadata["head_repository"]["full_name"] = serde_json::json!("O/R");
    assert!(resolve_mock(&plan, &parent, metadata.clone()).0.is_some());
    assert!(authentic_attempt(
        &metadata.to_string(),
        &parent,
        "o/r",
        "testmain",
        ".github/workflows/ci.yml"
    ));
}
