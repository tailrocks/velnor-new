//! Required acquires the plan-selected proof despite a newer same-base run.

use super::*;
use crate::cover::revalidate::cover_revalidate_fixtures::{manifest_for, plan_for};
use velnor_actions_contract::{BaselineProof, PlanBaseline, canonical_json_bytes, digest_b3};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

fn selected_fixture() -> (Plan, crate::merge::BaselineManifest) {
    let base = "1".repeat(40);
    let mut parent = manifest_for(&base);
    parent.repository_id = digest_b3(b"github.com/o/r");
    let mut plan = plan_for(&parent, Some(&base));
    parent.compatibility_id = crate::cover_compat::baseline_compat_for_plan(&plan).expect("compat");
    parent.artifact_name =
        velnor_actions_contract::artifact_id_for_baseline(&base, &parent.compatibility_id)
            .expect("name");
    parent.artifact_id = crate::cover_compat::baseline_artifact_numeric_id(&parent.artifact_name);
    let bytes = canonical_json_bytes(&parent).expect("selected bytes");
    plan.baseline = PlanBaseline::used(
        &base,
        7,
        parent.artifact_id,
        &parent.artifact_name,
        &digest_b3(&bytes),
    )
    .expect("selected baseline");
    plan.obligations[0].baseline_proof = Some(
        BaselineProof::new(
            &base,
            7,
            parent.artifact_id,
            &parent.artifact_name,
            &digest_b3(&bytes),
        )
        .expect("selected obligation proof"),
    );
    plan.validate().expect("valid selected plan");
    (plan, parent)
}

fn zipped_manifest(parent: &crate::merge::BaselineManifest) -> Vec<u8> {
    use std::io::Write as _;
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

#[test]
fn required_preserves_plan_selected_run_and_digest_after_newer_run() {
    let (plan, parent) = selected_fixture();
    let base = parent.source_commit.clone();
    let bytes = canonical_json_bytes(&parent).expect("selected bytes");
    let archive = zipped_manifest(&parent);
    let value = serde_json::to_value(&plan).expect("downloaded plan");
    let required = tempfile::tempdir().expect("required stage");
    let mut calls = Vec::new();
    let staged = retrieve_baseline_using(required.path(), &value, |typed| {
        let selected = planned::resolve_planned(
            typed,
            "o/r",
            "testmain",
            |args| {
                let args: Vec<_> = args
                    .iter()
                    .map(|arg| arg.to_string_lossy().into_owned())
                    .collect();
                calls.push(args.clone());
                if args[1].ends_with("/artifacts") {
                    Ok(serde_json::json!([{"total_count":1,"artifacts":[{
                    "id":99,"name":parent.artifact_name,"expired":false,
                    "size_in_bytes":archive.len(),"digest":format!("sha256:{}",sha256(&archive)),
                    "workflow_run":{"id":7,"head_sha":base,"head_branch":"testmain"}
                }]}]).to_string())
                } else {
                    assert!(args[1].ends_with("/runs/7/attempts/1"));
                    Ok(serde_json::json!({"id":7,"run_attempt":1,"head_sha":base,
                    "event":"push","status":"completed","conclusion":"success",
                    "head_branch":"testmain","path":".github/workflows/ci.yml",
                    "repository":{"full_name":"o/r"},
                    "head_repository":{"full_name":"o/r"}})
                    .to_string())
                }
            },
            |_, receipt| {
                assert_eq!(receipt.service_id, 99);
                Ok(archive.clone())
            },
        )
        .expect("selected authenticated parent");
        stage_manifest(required.path(), selected.manifest())
    });
    assert!(staged);
    assert_eq!(
        std::fs::read(required.path().join("baseline.json")).expect("staged proof"),
        bytes
    );
    assert!(!calls.iter().any(|args| args[1] == "list"));
    assert!(!retrieve_baseline_using(
        required.path(),
        &value,
        |_| panic!("staged wins")
    ));
}
