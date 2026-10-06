//! Paired named-check matrix rows bind lane, report, and artifact identities.

use super::*;
use crate::workflow::execute::{ExecuteTaskIds, ExecuteTaskRef};
use std::collections::BTreeMap;

const TASK: &str = "stack/mise/demo/check/default";

fn entry(variant: NamedCheckLaneVariant, job_id: &str) -> MatrixEntry {
    let runs_on = match variant {
        NamedCheckLaneVariant::Hosted => "ubuntu-26.04",
        NamedCheckLaneVariant::ScaleSet => "scale-set:velnor+ubuntu-26.04-scale-set",
    };
    MatrixEntry::derive_for_lane(
        "mise",
        TASK,
        "mise check:all",
        &format!("b3-{}", "a".repeat(64)),
        serde_json::json!({"check_id":"demo"}),
        ExecuteTaskIds {
            tasks: BTreeMap::from([("check".to_owned(), ExecuteTaskRef::Single(TASK.into()))]),
        },
        &format!("b3-{}", "b".repeat(64)),
        "local",
        job_id,
        Some(variant),
        PlannedPlatform::new(runs_on, "x86_64-unknown-linux-gnu").expect("planned platform"),
    )
    .expect("derived paired lane")
}

#[test]
fn hosted_and_scale_set_rows_have_distinct_matrix_report_and_artifact_ids() {
    let hosted = entry(NamedCheckLaneVariant::Hosted, "check-demo__hosted");
    let local = entry(NamedCheckLaneVariant::ScaleSet, "check-demo__local");
    hosted.validate("local").expect("hosted lane identity");
    local.validate("local").expect("Scale Set lane identity");
    assert_ne!(hosted.id, local.id);
    assert_ne!(hosted.matrix_key, local.matrix_key);
    assert_ne!(hosted.report_id, local.report_id);
    assert_ne!(hosted.artifact_id, local.artifact_id);
    assert_eq!(hosted.task_id, local.task_id);
    assert_eq!(hosted.lane_variant, Some(NamedCheckLaneVariant::Hosted));
    assert_eq!(local.lane_variant, Some(NamedCheckLaneVariant::ScaleSet));
}

#[test]
fn lane_identity_rejects_wrong_stack_or_emitted_job_suffix() {
    let error = MatrixEntry::derive_for_lane(
        "mise",
        TASK,
        "mise check:all",
        &format!("b3-{}", "a".repeat(64)),
        serde_json::json!({}),
        ExecuteTaskIds::default(),
        &format!("b3-{}", "b".repeat(64)),
        "local",
        "check-demo__local",
        Some(NamedCheckLaneVariant::Hosted),
        PlannedPlatform::new("ubuntu-26.04", "x86_64-unknown-linux-gnu").expect("planned platform"),
    )
    .expect_err("hosted variant cannot claim the local job");
    assert!(
        error.to_string().contains("lane_job_id_mismatch"),
        "{error}"
    );
}
