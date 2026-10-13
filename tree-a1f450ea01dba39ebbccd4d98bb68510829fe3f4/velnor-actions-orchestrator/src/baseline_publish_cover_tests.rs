//! Trusted-manifest staging and covered-task carry-forward tests.
//!
//! Declared via `#[path]` from `baseline_publish.rs` under `cfg(test)`.

use super::baseline_publish_tests::{fixture_plan, request_json, staged_manifest, staged_run};
use super::*;
use velnor_actions_contract::artifact_id_for_baseline;

#[test]
fn publish_stages_trusted_manifest_under_derived_name() {
    let head = "a".repeat(40);
    let plan = fixture_plan(&head, "r7-a1");
    let temp = staged_run(&plan, "r7-a1");
    let outputs = baseline_publish_to(&request_json(&head), "r7-a1", temp.path()).expect("publish");
    let compat = crate::cover_compat::baseline_compat_for_plan(&plan).expect("compat");
    let expected = artifact_id_for_baseline(&head, &compat).expect("name");
    assert_eq!(outputs.artifact_name, expected);
    let staged = staged_manifest(temp.path(), "r7-a1");
    assert_eq!(staged["schema"], 2);
    assert_eq!(staged["source_commit"], head);
    assert_eq!(staged["ref"], "refs/heads/testmain");
    assert_eq!(staged["event"], "push");
    assert_eq!(
        staged["workflow_ref"],
        "o/r/.github/workflows/ci.yml@refs/heads/testmain"
    );
    assert_eq!(staged["run_id"], 7);
    assert_eq!(staged["run_attempt"], 1);
    assert_eq!(staged["final_status"], "passed");
    assert_eq!(staged["compatibility_id"], compat);
    assert_eq!(staged["artifact_name"], expected);
    assert_eq!(
        staged["artifact_id"],
        crate::cover_compat::baseline_artifact_numeric_id(&expected)
    );
    assert_eq!(staged["tasks"].as_array().expect("tasks").len(), 2);
}
