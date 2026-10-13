//! Required acquires the plan-selected proof despite a newer same-base run.

use super::*;
use crate::cover::revalidate::cover_revalidate_fixtures::{manifest_for, plan_for};
use std::cell::RefCell;
use velnor_actions_contract::{BaselineProof, PlanBaseline, canonical_json_bytes, digest_b3};

fn selected_fixture() -> (Plan, crate::merge::BaselineManifest) {
    let base = "1".repeat(40);
    let mut parent = manifest_for(&base);
    parent.repository_id = digest_b3(b"github.com/o/r");
    let mut plan = plan_for(&parent, Some(&base));
    parent.compatibility_id = crate::cover_compat::baseline_compat_for_plan(&plan).expect("compat");
    parent.artifact_name = crate::cover_baseline::lookup_artifact_name(&plan, &base).expect("name");
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

#[test]
fn required_preserves_plan_selected_run_and_digest_after_newer_run() {
    let (plan, parent) = selected_fixture();
    let base = parent.source_commit.clone();
    let bytes = canonical_json_bytes(&parent).expect("selected bytes");
    let archive = crate::cover::shard_baseline::archive::test_archive("baseline.json", &bytes);
    let value = serde_json::to_value(&plan).expect("downloaded plan");
    let required = tempfile::tempdir().expect("required stage");
    let calls = RefCell::new(Vec::new());
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
                calls.borrow_mut().push(args.clone());
                if args[1].contains("/artifacts?name=") {
                    return Ok(serde_json::json!({"total_count":1,"artifacts":[{"id":99,
                        "name":parent.artifact_name,"expired":false,
                        "size_in_bytes":archive.len(),"digest":format!("sha256:{}",
                            crate::cover_identity::generator::sha256_hex(&archive))}]})
                    .to_string());
                }
                assert!(args[1].ends_with("/runs/7/attempts/1"));
                Ok(serde_json::json!({"id":7,"run_attempt":1,"head_sha":base,
                    "event":"push","status":"completed","conclusion":"success",
                    "head_branch":"testmain","path":".github/workflows/ci.yml",
                    "repository":{"full_name":"o/r"}})
                .to_string())
            },
            |args| {
                let args: Vec<_> = args
                    .iter()
                    .map(|arg| arg.to_string_lossy().into_owned())
                    .collect();
                calls.borrow_mut().push(args.clone());
                assert_eq!(args, ["api", "repos/o/r/actions/artifacts/99/zip"]);
                Ok(archive.clone())
            },
        )
        .expect("selected authenticated parent");
        stage_manifest(required.path(), &selected)
    });
    let calls = calls.into_inner();
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
