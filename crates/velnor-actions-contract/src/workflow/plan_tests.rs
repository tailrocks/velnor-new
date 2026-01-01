//! Matrix topology rejects incomplete paired named-check proof sets.

use super::*;
use crate::workflow::execute::ExecuteTaskIds;
use crate::workflow::platform::PlannedPlatform;

const TASK: &str = "stack/mise/demo/check/default";

fn lane(variant: NamedCheckLaneVariant, job_id: &str) -> MatrixEntry {
    let runs_on = match variant {
        NamedCheckLaneVariant::Hosted => "ubuntu-26.04",
        NamedCheckLaneVariant::ScaleSet => "scale-set:velnor+ubuntu-26.04-scale-set",
    };
    MatrixEntry::derive_for_lane(
        "mise",
        TASK,
        "mise check:all",
        &format!("b3-{}", "a".repeat(64)),
        serde_json::json!({}),
        ExecuteTaskIds::default(),
        &format!("b3-{}", "b".repeat(64)),
        "local",
        job_id,
        Some(variant),
        PlannedPlatform::new(runs_on, "x86_64-unknown-linux-gnu").expect("planned platform"),
    )
    .expect("lane entry")
}

#[test]
fn paired_checks_require_exactly_one_entry_for_each_lane() {
    let hosted = lane(NamedCheckLaneVariant::Hosted, "check-demo__hosted");
    let local = lane(NamedCheckLaneVariant::ScaleSet, "check-demo__local");
    assert!(validate_named_check_lane_pairs(&[hosted.clone(), local.clone()]).is_ok());
    assert!(validate_named_check_lane_pairs(std::slice::from_ref(&hosted)).is_err());
    assert!(validate_named_check_lane_pairs(&[hosted.clone(), hosted]).is_err());
    assert!(validate_named_check_lane_pairs(std::slice::from_ref(&local)).is_err());
    assert!(validate_named_check_lane_pairs(&[local.clone(), local]).is_err());
}

#[test]
fn paired_and_single_entries_cannot_share_one_logical_task() {
    let hosted = lane(NamedCheckLaneVariant::Hosted, "check-demo__hosted");
    let local = lane(NamedCheckLaneVariant::ScaleSet, "check-demo__local");
    let mut single = MatrixEntry::derive(
        "mise",
        TASK,
        "mise check:all",
        &format!("b3-{}", "a".repeat(64)),
        serde_json::json!({}),
        ExecuteTaskIds::default(),
        &format!("b3-{}", "b".repeat(64)),
        "local",
        "check-demo",
        PlannedPlatform::new("ubuntu-26.04", "x86_64-unknown-linux-gnu").expect("planned platform"),
    )
    .expect("single entry");
    single.task_id = TASK.to_owned();
    assert!(validate_named_check_lane_pairs(&[hosted, local, single]).is_err());
}
