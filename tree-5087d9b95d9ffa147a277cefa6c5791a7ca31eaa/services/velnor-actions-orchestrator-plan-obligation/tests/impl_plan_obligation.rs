use std::collections::BTreeMap;

use velnor_actions_contract_workflow::WorkflowEvent;
use velnor_actions_mise::ToolCatalog;
use velnor_actions_orchestrator_graph::internal_plan::snapshot::ExecutionSnapshot;
use velnor_actions_orchestrator_graph::internal_plan::wire_w2::GroupWire;
use velnor_actions_orchestrator_plan_obligation::plan_obligation::{GroupInputs, plan_group};

use super::support::{LABEL, discovery_with, planned, rust_proposal, tofu_proposal, tofu_root};

#[test]
fn changed_tofu_task_plans_execute_obligation() {
    use velnor_actions_contract_workflow::ObligationDecision;
    use velnor_actions_tofu_core::TofuTaskKind;
    let task = tofu_proposal(TofuTaskKind::Validate);
    let discovery = discovery_with(vec![task.clone()]);
    let snapshot = ExecutionSnapshot::build(&discovery);
    let catalog = ToolCatalog::pinned();
    let generator = velnor_actions_orchestrator_graph::internal_plan::default_generator();
    let lanes = BTreeMap::new();
    let root = tofu_root();
    let (obligation, entries) = planned(
        &task,
        &discovery,
        &snapshot,
        &catalog,
        &generator,
        &lanes,
        root.path(),
        true,
    );
    assert_eq!(obligation.task_id, task.task_id);
    assert_eq!(obligation.decision, ObligationDecision::Execute);
    assert_eq!(obligation.reason, "affected_by_change");
    assert!(!obligation.task_digest.is_empty());
    assert!(!obligation.input_digest.is_empty());
    assert!(!obligation.closure_digest.is_empty());
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].task_id, task.task_id);
}

#[test]
fn planned_entry_records_lane_target_dir() {
    use velnor_actions_tofu_core::TofuTaskKind;
    let task = tofu_proposal(TofuTaskKind::Validate);
    let discovery = discovery_with(vec![task.clone()]);
    let snapshot = ExecutionSnapshot::build(&discovery);
    let catalog = ToolCatalog::pinned();
    let generator = velnor_actions_orchestrator_graph::internal_plan::default_generator();
    let lanes = BTreeMap::new();
    let root = tofu_root();
    let (_, entries) = planned(
        &task,
        &discovery,
        &snapshot,
        &catalog,
        &generator,
        &lanes,
        root.path(),
        true,
    );
    let lane = entries[0]
        .cache_ids
        .as_ref()
        .expect("cache ids")
        .lane_id()
        .to_owned();
    let expected = velnor_actions_orchestrator_graph::internal_plan::target_dir_for_lane_id(&lane);
    assert_eq!(
        entries[0].adapter_metadata["cargo_target_dir"],
        serde_json::Value::String(expected)
    );
}

#[test]
fn planned_identities_are_deterministic() {
    use velnor_actions_tofu_core::TofuTaskKind;
    let task = tofu_proposal(TofuTaskKind::Validate);
    let discovery = discovery_with(vec![task.clone()]);
    let snapshot = ExecutionSnapshot::build(&discovery);
    let catalog = ToolCatalog::pinned();
    let generator = velnor_actions_orchestrator_graph::internal_plan::default_generator();
    let lanes = BTreeMap::new();
    let root = tofu_root();
    let plan_once = || {
        planned(
            &task,
            &discovery,
            &snapshot,
            &catalog,
            &generator,
            &lanes,
            root.path(),
            true,
        )
    };
    let (left, _) = plan_once();
    let (right, _) = plan_once();
    assert_eq!(left.task_digest, right.task_digest);
    assert_eq!(left.input_digest, right.input_digest);
    assert_eq!(left.closure_digest, right.closure_digest);
}

#[test]
fn missing_job_group_fails_crate_job_id_missing() {
    use velnor_actions_tofu_core::TofuTaskKind;
    let task = tofu_proposal(TofuTaskKind::Validate);
    let discovery = discovery_with(Vec::new());
    let snapshot = ExecutionSnapshot::build(&discovery);
    let catalog = ToolCatalog::pinned();
    let generator = velnor_actions_orchestrator_graph::internal_plan::default_generator();
    let lanes = BTreeMap::new();
    let root = tofu_root();
    let wire = GroupWire {
        event: WorkflowEvent::Push,
        generator: &generator,
    };
    let inputs = GroupInputs {
        discovery: &discovery,
        task: &task,
        run_key: "r7-a1",
        label: LABEL,
        lane: 0,
        catalog: &catalog,
        wire,
        changed: true,
        snapshot: &snapshot,
        root: root.path(),
        named_check_lanes: &lanes,
    };
    let err = plan_group(&inputs, &mut velnor_actions_tofu_core::FileCache::new())
        .expect_err("job id missing");
    assert!(err.to_string().contains("crate_job_id_missing"), "{err}");
}

#[test]
fn tofu_drift_fails_closed_through_public_path() {
    use velnor_actions_tofu_core::TofuTaskKind;
    let mut task = tofu_proposal(TofuTaskKind::InitForValidate);
    task.identity.compile_driver = "cargo".to_owned();
    let discovery = discovery_with(vec![task.clone()]);
    let snapshot = ExecutionSnapshot::build(&discovery);
    let catalog = ToolCatalog::pinned();
    let generator = velnor_actions_orchestrator_graph::internal_plan::default_generator();
    let lanes = BTreeMap::new();
    let root = tofu_root();
    let wire = GroupWire {
        event: WorkflowEvent::Push,
        generator: &generator,
    };
    let inputs = GroupInputs {
        discovery: &discovery,
        task: &task,
        run_key: "r7-a1",
        label: LABEL,
        lane: 0,
        catalog: &catalog,
        wire,
        changed: true,
        snapshot: &snapshot,
        root: root.path(),
        named_check_lanes: &lanes,
    };
    let err = plan_group(&inputs, &mut velnor_actions_tofu_core::FileCache::new())
        .expect_err("drift fails");
    assert!(err.to_string().contains("unknown_driver:cargo"), "{err}");
}

#[test]
fn unchanged_tofu_task_keeps_execute_without_changed_reason() {
    use velnor_actions_contract_workflow::ObligationDecision;
    use velnor_actions_tofu_core::TofuTaskKind;
    let task = tofu_proposal(TofuTaskKind::Validate);
    let discovery = discovery_with(vec![task.clone()]);
    let snapshot = ExecutionSnapshot::build(&discovery);
    let catalog = ToolCatalog::pinned();
    let generator = velnor_actions_orchestrator_graph::internal_plan::default_generator();
    let lanes = BTreeMap::new();
    let root = tofu_root();
    let (obligation, entries) = planned(
        &task,
        &discovery,
        &snapshot,
        &catalog,
        &generator,
        &lanes,
        root.path(),
        false,
    );
    assert_eq!(obligation.decision, ObligationDecision::Execute);
    assert_ne!(obligation.reason, "affected_by_change");
    assert_eq!(entries.len(), 1);
}

#[test]
fn changed_rust_task_plans_execute_obligation() {
    use velnor_actions_contract_workflow::ObligationDecision;
    let task = rust_proposal();
    let discovery = discovery_with(vec![task.clone()]);
    let snapshot = ExecutionSnapshot::build(&discovery);
    let catalog = ToolCatalog::pinned();
    let generator = velnor_actions_orchestrator_graph::internal_plan::default_generator();
    let lanes = BTreeMap::new();
    let root = tempfile::tempdir().expect("tempdir");
    let (obligation, entries) = planned(
        &task,
        &discovery,
        &snapshot,
        &catalog,
        &generator,
        &lanes,
        root.path(),
        true,
    );
    assert_eq!(obligation.task_id, task.task_id);
    assert_eq!(obligation.decision, ObligationDecision::Execute);
    assert_eq!(obligation.reason, "affected_by_change");
    assert_eq!(entries.len(), 1);
}

#[test]
fn rust_task_without_proposals_fails_crate_job_id_missing() {
    let task = rust_proposal();
    let discovery = discovery_with(Vec::new());
    let snapshot = ExecutionSnapshot::build(&discovery);
    let catalog = ToolCatalog::pinned();
    let generator = velnor_actions_orchestrator_graph::internal_plan::default_generator();
    let lanes = BTreeMap::new();
    let root = tempfile::tempdir().expect("tempdir");
    let wire = GroupWire {
        event: WorkflowEvent::Push,
        generator: &generator,
    };
    let inputs = GroupInputs {
        discovery: &discovery,
        task: &task,
        run_key: "r7-a1",
        label: LABEL,
        lane: 0,
        catalog: &catalog,
        wire,
        changed: true,
        snapshot: &snapshot,
        root: root.path(),
        named_check_lanes: &lanes,
    };
    let err = plan_group(&inputs, &mut velnor_actions_tofu_core::FileCache::new())
        .expect_err("job id missing");
    assert!(err.to_string().contains("crate_job_id_missing"), "{err}");
}
