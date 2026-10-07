use super::support::{rust_proposal, tofu_proposal};
use velnor_actions_orchestrator_plan_obligation::plan_obligation::lane_table;

#[test]
fn empty_universe_yields_empty_table() {
    assert!(lane_table(&[]).is_empty());
}

#[test]
fn single_task_takes_lane_zero() {
    let task = tofu_proposal(velnor_actions_tofu_core::TofuTaskKind::Validate);
    let table = lane_table(&[&task]);
    assert_eq!(table.len(), 1);
    assert_eq!(table[&task.task_id], 0);
}

#[test]
fn lanes_follow_sorted_task_id_order() {
    use velnor_actions_tofu_core::TofuTaskKind;
    let validate = tofu_proposal(TofuTaskKind::Validate);
    let init = tofu_proposal(TofuTaskKind::InitForValidate);
    let rust = rust_proposal();
    let mut ids = [&validate, &init, &rust].map(|task| task.task_id.clone());
    ids.sort();
    let table = lane_table(&[&rust, &validate, &init]);
    assert_eq!(table.len(), 3);
    for (lane, id) in ids.iter().enumerate() {
        assert_eq!(table[id], u32::try_from(lane).expect("lane fits"));
    }
}

#[test]
fn lanes_are_stable_across_calls() {
    use velnor_actions_tofu_core::TofuTaskKind;
    let validate = tofu_proposal(TofuTaskKind::Validate);
    let rust = rust_proposal();
    let universe = [&validate, &rust];
    assert_eq!(lane_table(&universe), lane_table(&universe));
}
