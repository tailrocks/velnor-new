//! Retrieve enumeration tests: plan order plus shared-artifact dedupe.
//!
//! Declared via `#[path]` from `retrieve_reports.rs` under `cfg(test)`.

use super::*;

#[test]
fn enumeration_follows_plan_order() {
    let plan = serde_json::json!({
        "matrix": {"include": [
            {"artifact_id": "velnor-matrix-r7-a2-m-0000000000000002"},
            {"artifact_id": "velnor-matrix-r7-a2-m-0000000000000001"},
        ]}
    });
    assert_eq!(
        expected_artifact_ids(&plan),
        [
            "velnor-matrix-r7-a2-m-0000000000000002",
            "velnor-matrix-r7-a2-m-0000000000000001",
        ]
    );
    assert_eq!(
        expected_artifact_ids(&serde_json::json!({})),
        [] as [&str; 0]
    );
}

#[test]
fn enumeration_dedupes_shared_job_artifacts() {
    let plan = serde_json::json!({
        "matrix": {"include": [
            {"artifact_id": "velnor-crate-r7-a2-crate_a"},
            {"artifact_id": "velnor-crate-r7-a2-crate_a"},
            {"artifact_id": "velnor-crate-r7-a2-plan"},
        ]}
    });
    assert_eq!(
        expected_artifact_ids(&plan),
        ["velnor-crate-r7-a2-crate_a", "velnor-crate-r7-a2-plan",]
    );
}
