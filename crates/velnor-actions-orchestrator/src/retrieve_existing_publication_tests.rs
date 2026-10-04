//! Existing current-run publications retain exact service authority.

use std::collections::BTreeMap;
use std::io::Write as _;

use super::*;
use crate::cover::revalidate::cover_revalidate_fixtures::{manifest_for, plan_for};
use crate::cover::shard_baseline::AcquiredBaseline;
use crate::merge::BaselineManifest;
use velnor_actions_contract::{canonical_json_bytes, digest_b3, run_key_for_ci};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

fn fixture(attempt: u64) -> (Plan, BaselineManifest) {
    let base = "1".repeat(40);
    let mut parent = manifest_for(&base);
    parent.repository_id = digest_b3(b"github.com/o/r");
    let mut plan = plan_for(&parent, Some(&base));
    plan.head = base.clone();
    plan.run_key = run_key_for_ci(7, attempt);
    parent.compatibility_id =
        crate::cover_compat::baseline_compat_for_plan(&plan).expect("compatibility");
    parent.artifact_name =
        velnor_actions_contract::artifact_id_for_baseline(&base, &parent.compatibility_id)
            .expect("source/compatibility name");
    parent.artifact_id = crate::cover_compat::baseline_artifact_numeric_id(&parent.artifact_name);
    (plan, parent)
}

fn zipped_manifest(parent: &BaselineManifest) -> Vec<u8> {
    let bytes = canonical_json_bytes(parent).expect("canonical manifest");
    let mut writer = ZipWriter::new(std::io::Cursor::new(Vec::new()));
    writer
        .start_file(
            "baseline.json",
            SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
        )
        .expect("start member");
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

fn attempt_record(parent: &BaselineManifest, conclusion: &str) -> serde_json::Value {
    serde_json::json!({
        "id": parent.run_id,
        "run_attempt": parent.run_attempt,
        "head_sha": parent.source_commit,
        "event": "push",
        "status": "completed",
        "conclusion": conclusion,
        "head_branch": "testmain",
        "path": ".github/workflows/ci.yml",
        "repository": {"full_name": "o/r"},
        "head_repository": {"full_name": "o/r"}
    })
}

fn service_artifact(parent: &BaselineManifest, bytes: &[u8], service_id: u64) -> serde_json::Value {
    serde_json::json!({
        "id": service_id,
        "name": parent.artifact_name,
        "expired": false,
        "size_in_bytes": bytes.len(),
        "digest": format!("sha256:{}", sha256(bytes)),
        "workflow_run": {
            "id": parent.run_id,
            "head_sha": parent.source_commit,
            "head_branch": "testmain"
        }
    })
}

fn resolve_mock(
    plan: &Plan,
    artifacts: Vec<serde_json::Value>,
    attempt_records: BTreeMap<u64, serde_json::Value>,
    manifests: BTreeMap<u64, Vec<u8>>,
) -> Result<Option<AcquiredBaseline>, String> {
    resolve_existing(
        plan,
        7,
        "o/r",
        "testmain",
        |args| {
            let endpoint = args[1].to_string_lossy();
            if endpoint.ends_with("/artifacts") {
                return Ok(serde_json::json!([{
                    "total_count": artifacts.len(),
                    "artifacts": artifacts,
                }])
                .to_string());
            }
            let attempt = endpoint
                .rsplit_once('/')
                .and_then(|(_, value)| value.parse::<u64>().ok())
                .ok_or_else(|| "bad_test_endpoint".to_owned())?;
            attempt_records
                .get(&attempt)
                .map(serde_json::Value::to_string)
                .ok_or_else(|| "no_test_attempt".to_owned())
        },
        |_, receipt| {
            manifests
                .get(&receipt.service_id)
                .cloned()
                .ok_or_else(|| "no_test_archive".to_owned())
        },
    )
}

#[test]
fn retry_falls_back_to_prior_successful_attempt_and_retains_receipt() {
    let (plan, first) = fixture(2);
    let first_bytes = zipped_manifest(&first);
    let mut retry = first.clone();
    retry.run_attempt = 2;
    retry.artifact_name = velnor_actions_contract::artifact_id_for_baseline(
        &retry.source_commit,
        &retry.compatibility_id,
    )
    .expect("retry name");
    retry.artifact_id = crate::cover_compat::baseline_artifact_numeric_id(&retry.artifact_name);
    let retry_bytes = zipped_manifest(&retry);
    let artifacts = vec![
        service_artifact(&first, &first_bytes, 501),
        service_artifact(&retry, &retry_bytes, 502),
    ];
    let mut records = BTreeMap::new();
    records.insert(2, attempt_record(&retry, "failure"));
    records.insert(1, attempt_record(&first, "success"));
    let mut manifests = BTreeMap::new();
    manifests.insert(501, first_bytes);
    manifests.insert(502, retry_bytes);

    let acquired = resolve_mock(&plan, artifacts, records, manifests)
        .expect("complete lookup")
        .expect("prior successful attempt");
    assert_eq!(acquired.manifest().run_attempt, 1);
}

#[test]
fn missing_digest_refuses_existing_publication() {
    let (plan, parent) = fixture(2);
    let bytes = zipped_manifest(&parent);
    let mut artifact = service_artifact(&parent, &bytes, 501);
    artifact["digest"] = serde_json::Value::Null;
    let mut records = BTreeMap::new();
    records.insert(1, attempt_record(&parent, "success"));
    let mut manifests = BTreeMap::new();
    manifests.insert(501, bytes);
    assert!(resolve_mock(&plan, vec![artifact], records, manifests).is_err());
}

#[test]
fn valid_complete_listing_without_artifact_is_absence() {
    let (plan, _) = fixture(2);
    assert!(
        resolve_mock(&plan, Vec::new(), BTreeMap::new(), BTreeMap::new())
            .is_ok_and(|found| found.is_none())
    );
}
