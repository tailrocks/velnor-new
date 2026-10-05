//! Required acquires the plan-selected proof despite a newer same-base run.

use super::*;
use crate::cover::revalidate::cover_revalidate_fixtures::{manifest_for, plan_for};
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
    let value = serde_json::to_value(&plan).expect("downloaded plan");
    let required = tempfile::tempdir().expect("required stage");
    let mut calls = Vec::new();
    let staged = retrieve_baseline_using(required.path(), &value, |typed| {
        let selected = planned::resolve_planned(typed, "o/r", "testmain", |args| {
            let args: Vec<_> = args
                .iter()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect();
            calls.push(args.clone());
            if args[0] == "run" && args[1] == "list" {
                return Ok(serde_json::json!([{"databaseId":8,"headSha":base,
                    "event":"push","conclusion":"success","headBranch":"testmain",
                    "attempt":1}])
                .to_string());
            }
            if args[0] == "run" {
                assert_eq!(args[2], "7", "newer run never replaces selected run");
                let at = args
                    .iter()
                    .position(|arg| arg == "--dir")
                    .expect("dir flag")
                    + 1;
                let destination = Path::new(&args[at]);
                std::fs::create_dir(destination).expect("download dir");
                std::fs::write(destination.join("baseline.json"), &bytes).expect("download");
                Ok(String::new())
            } else if args[1].ends_with("/artifacts") {
                Ok(serde_json::json!({"artifacts":[{"id":99,
                    "name":parent.artifact_name,"expired":false}]})
                .to_string())
            } else {
                assert!(args[1].ends_with("/runs/7/attempts/1"));
                Ok(serde_json::json!({"id":7,"run_attempt":1,"head_sha":base,
                    "event":"push","status":"completed","conclusion":"success",
                    "head_branch":"testmain","path":".github/workflows/ci.yml",
                    "repository":{"full_name":"o/r"}})
                .to_string())
            }
        })
        .expect("selected authenticated parent");
        stage_manifest(required.path(), &selected)
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
