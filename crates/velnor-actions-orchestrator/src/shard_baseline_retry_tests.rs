//! Run and attempt identity come from summaries, API receipts, and manifest.

use std::io::Write as _;

use super::*;
use crate::cover::revalidate::cover_revalidate_fixtures::manifest_for;
use crate::merge::BaselineManifest;
use velnor_actions_contract::{canonical_json_bytes, digest_b3};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

fn proof() -> BaselineManifest {
    let mut manifest = manifest_for(&"1".repeat(40));
    manifest.repository_id = digest_b3(b"github.com/o/r");
    manifest.compatibility_id = digest_b3(b"baseline compatibility");
    manifest.artifact_name = velnor_actions_contract::artifact_id_for_baseline(
        &manifest.source_commit,
        &manifest.compatibility_id,
    )
    .expect("artifact name");
    manifest.artifact_id =
        crate::cover_compat::baseline_artifact_numeric_id(&manifest.artifact_name);
    manifest
}

fn zipped_manifest(manifest: &BaselineManifest) -> Vec<u8> {
    let payload = canonical_json_bytes(manifest).expect("canonical manifest");
    let mut writer = ZipWriter::new(std::io::Cursor::new(Vec::new()));
    writer
        .start_file(
            "baseline.json",
            SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
        )
        .expect("start member");
    writer.write_all(&payload).expect("write manifest");
    writer.finish().expect("finish ZIP").into_inner()
}

fn sha256(bytes: &[u8]) -> String {
    use sha2::{Digest as _, Sha256};
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn exact_attempt(manifest: &BaselineManifest, conclusion: &str) -> serde_json::Value {
    serde_json::json!({
        "id": manifest.run_id,
        "run_attempt": manifest.run_attempt,
        "head_sha": manifest.source_commit,
        "event": "push",
        "status": "completed",
        "conclusion": conclusion,
        "head_branch": "testmain",
        "path": ".github/workflows/ci.yml@testmain",
        "repository": {"full_name": "o/r"},
        "head_repository": {"full_name": "o/r"}
    })
}

fn service_artifact(
    manifest: &BaselineManifest,
    bytes: &[u8],
    service_id: u64,
) -> serde_json::Value {
    serde_json::json!({
        "id": service_id,
        "name": manifest.artifact_name,
        "expired": false,
        "size_in_bytes": bytes.len(),
        "digest": format!("sha256:{}", sha256(bytes)),
        "workflow_run": {
            "id": manifest.run_id,
            "head_sha": manifest.source_commit,
            "head_branch": "testmain"
        }
    })
}

fn lookup_manifest(
    manifest: &BaselineManifest,
    summary_conclusion: &str,
    summary_attempt: u64,
) -> (Result<AcquiredBaseline, String>, Vec<Vec<String>>) {
    lookup_manifest_with_attempt_response(manifest, summary_conclusion, summary_attempt, None)
}

fn lookup_manifest_with_attempt_response(
    manifest: &BaselineManifest,
    summary_conclusion: &str,
    summary_attempt: u64,
    attempt_response: Option<serde_json::Value>,
) -> (Result<AcquiredBaseline, String>, Vec<Vec<String>>) {
    let lookup = BaselineLookup::new(
        &manifest.source_commit,
        ".github/workflows/ci.yml",
        "testmain",
        "o/r",
    )
    .expect("lookup");
    let compatibility = manifest.compatibility_id.clone();
    let bytes = zipped_manifest(manifest);
    let artifact = service_artifact(manifest, &bytes, 99);
    let mut calls = Vec::new();
    let acquired = resolve_with(
        &lookup,
        &manifest.artifact_name,
        &compatibility,
        |args| {
            let args: Vec<String> = args
                .iter()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect();
            calls.push(args.clone());
            if args[0] == "run" && args[1] == "list" {
                Ok(serde_json::json!([{
                    "databaseId": 7,
                    "headSha": manifest.source_commit,
                    "event": "push",
                    "conclusion": summary_conclusion,
                    "headBranch": "testmain",
                    "attempt": summary_attempt
                }])
                .to_string())
            } else if args[1].ends_with("/artifacts") {
                Ok(serde_json::json!([{
                    "total_count": 1,
                    "artifacts": [artifact.clone()]
                }])
                .to_string())
            } else {
                assert_eq!(
                    args[1],
                    format!("repos/o/r/actions/runs/7/attempts/{}", manifest.run_attempt)
                );
                Ok(attempt_response
                    .clone()
                    .unwrap_or_else(|| exact_attempt(manifest, "success"))
                    .to_string())
            }
        },
        |_| Ok(bytes.clone()),
    );
    (acquired, calls)
}

fn lookup_manifests(
    manifests: &[BaselineManifest],
    summary_attempt: u64,
) -> (Result<AcquiredBaseline, String>, Vec<Vec<String>>) {
    let first = manifests.first().expect("at least one manifest");
    let lookup = BaselineLookup::new(
        &first.source_commit,
        ".github/workflows/ci.yml",
        "testmain",
        "o/r",
    )
    .expect("lookup");
    let compatibility = first.compatibility_id.clone();
    let candidates: Vec<_> = manifests
        .iter()
        .enumerate()
        .map(|(index, manifest)| {
            let service_id = 100 + u64::try_from(index).expect("small test index");
            let bytes = zipped_manifest(manifest);
            let artifact = service_artifact(manifest, &bytes, service_id);
            (service_id, bytes, artifact)
        })
        .collect();
    let artifacts: Vec<_> = candidates
        .iter()
        .map(|(_, _, artifact)| artifact.clone())
        .collect();
    let listing = serde_json::json!([{
        "total_count": artifacts.len(),
        "artifacts": artifacts,
    }])
    .to_string();
    let mut calls = Vec::new();
    let acquired = resolve_with(
        &lookup,
        &first.artifact_name,
        &compatibility,
        |args| {
            let args: Vec<String> = args
                .iter()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect();
            calls.push(args.clone());
            if args[0] == "run" && args[1] == "list" {
                Ok(serde_json::json!([{
                    "databaseId": first.run_id,
                    "headSha": first.source_commit,
                    "event": "push",
                    "conclusion": "success",
                    "headBranch": "testmain",
                    "attempt": summary_attempt
                }])
                .to_string())
            } else if args[1].ends_with("/artifacts") {
                Ok(listing.clone())
            } else {
                let attempt = args[1]
                    .rsplit('/')
                    .next()
                    .and_then(|value| value.parse::<u64>().ok())
                    .ok_or_else(|| "bad_test_endpoint".to_owned())?;
                manifests
                    .iter()
                    .find(|manifest| manifest.run_attempt == attempt)
                    .map(|manifest| exact_attempt(manifest, "success").to_string())
                    .ok_or_else(|| "no_test_attempt".to_owned())
            }
        },
        |receipt| {
            candidates
                .iter()
                .find(|(service_id, _, _)| *service_id == receipt.service_id)
                .map(|(_, bytes, _)| bytes.clone())
                .ok_or_else(|| "no_test_archive".to_owned())
        },
    );
    (acquired, calls)
}

#[test]
fn exact_successful_attempt_authenticates_stable_name_artifact() {
    let manifest = proof();
    let original_bytes = canonical_json_bytes(&manifest).expect("original bytes");
    let (acquired, calls) = lookup_manifest(&manifest, "success", 1);
    let acquired = acquired.expect("authenticated selected attempt");
    assert_eq!(acquired.manifest().run_attempt, 1);
    assert_eq!(acquired.attempt_receipt().run_attempt, 1);
    assert_eq!(
        canonical_json_bytes(acquired.manifest()).expect("retrieved bytes"),
        original_bytes
    );
    assert_eq!(calls[2][1], "repos/o/r/actions/runs/7/attempts/1");
}

#[test]
fn retry_reuses_authenticated_original_attempt() {
    let manifest = proof();
    let (acquired, calls) = lookup_manifest(&manifest, "success", 2);
    let acquired = acquired.expect("original successful attempt remains eligible");
    assert_eq!(acquired.manifest().run_attempt, 1);
    assert_eq!(acquired.attempt_receipt().run_attempt, 1);
    assert_eq!(calls[2][1], "repos/o/r/actions/runs/7/attempts/1");
}

#[test]
fn two_successful_attempts_select_newest_and_ignore_older_ambiguity() {
    let mut first = proof();
    first.run_attempt = 1;
    let mut duplicate_first = first.clone();
    duplicate_first.run_attempt = 1;
    let mut second = first.clone();
    second.run_attempt = 2;

    let (acquired, calls) = lookup_manifests(&[first, duplicate_first, second], 2);
    let acquired = acquired.expect("newest authenticated success wins");
    assert_eq!(acquired.manifest().run_attempt, 2);
    assert_eq!(acquired.attempt_receipt().run_attempt, 2);
    assert_eq!(calls.len(), 5, "each listed artifact is authenticated");
    assert_eq!(calls[2][1], "repos/o/r/actions/runs/7/attempts/1");
    assert_eq!(calls[3][1], "repos/o/r/actions/runs/7/attempts/1");
    assert_eq!(calls[4][1], "repos/o/r/actions/runs/7/attempts/2");
}

#[test]
fn multiple_successes_at_newest_attempt_remain_ambiguous() {
    let first = proof();
    let mut second = first.clone();
    second.run_attempt = 2;
    let duplicate_second = second.clone();

    let (acquired, _) = lookup_manifests(&[first, second, duplicate_second], 2);
    assert_eq!(
        acquired.expect_err("two authenticated artifacts at selected attempt"),
        "baseline_artifact_ambiguous"
    );
}

#[test]
fn future_manifest_attempt_is_rejected() {
    let mut manifest = proof();
    manifest.run_attempt = 3;
    let (acquired, calls) = lookup_manifest(&manifest, "success", 2);
    assert_eq!(
        acquired.expect_err("future attempt"),
        "baseline_unavailable"
    );
    assert_eq!(calls.len(), 2, "future attempt rejects before API lookup");
}

#[test]
fn exact_attempt_api_must_match_manifest_attempt() {
    let manifest = proof();
    let mut response = exact_attempt(&manifest, "success");
    response["run_attempt"] = 2.into();
    let (acquired, calls) =
        lookup_manifest_with_attempt_response(&manifest, "success", 2, Some(response));
    assert_eq!(
        acquired.expect_err("mismatched attempt receipt"),
        "baseline_unavailable"
    );
    assert_eq!(calls[2][1], "repos/o/r/actions/runs/7/attempts/1");
}

#[test]
fn zero_manifest_attempt_is_rejected() {
    let mut manifest = proof();
    manifest.run_attempt = 0;
    let (acquired, calls) = lookup_manifest(&manifest, "success", 1);
    assert_eq!(
        acquired.expect_err("invalid attempt"),
        "baseline_unavailable"
    );
    assert_eq!(calls.len(), 2, "invalid attempt rejects before API lookup");
}

#[test]
fn failed_run_summary_cannot_authorize_baseline_artifact() {
    let manifest = proof();
    let (acquired, calls) = lookup_manifest(&manifest, "failure", 1);
    assert!(acquired.is_err());
    assert_eq!(calls.len(), 1, "failed summary never reaches artifact API");
}
