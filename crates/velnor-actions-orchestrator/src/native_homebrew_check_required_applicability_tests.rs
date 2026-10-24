use super::{
    actual_native_checks, require_native_authority, route_required_admission,
    validate_required_matrix_entry,
};
use std::cell::Cell;
use std::collections::BTreeMap;
use velnor_actions_contract::{
    ExecuteTaskIds, ExecuteTaskRef, MatrixEntry, Plan, PlanGenerator, PlanMatrix, PlanRunner,
    PlannedPlatform, RunnerSelection, Trust, WorkflowEvent,
    config::{
        CheckExecutor, CheckPlatform, CheckRunner, NativeHomebrewCheck, NativeHomebrewOperation,
    },
    plan_id_for_run,
};

fn plan(event: WorkflowEvent, entries: Vec<MatrixEntry>) -> Plan {
    Plan {
        schema: Plan::SCHEMA,
        run_key: "local".to_owned(),
        plan_id: plan_id_for_run("local").expect("plan ID"),
        base: None,
        head: "a".repeat(40),
        event,
        qualification: None,
        runner: PlanRunner {
            label: "ubuntu-26.04".to_owned(),
            selection: RunnerSelection::LatestDefault,
        },
        trust: Trust::Pr,
        generator: PlanGenerator {
            version: "1.0.0".to_owned(),
            target: "x86_64-unknown-linux-gnu".to_owned(),
            sha256: "b".repeat(64),
        },
        packages: Vec::new(),
        obligations: Vec::new(),
        matrix: PlanMatrix { include: entries },
        task_ids: Vec::new(),
        jobs: Vec::new(),
        warnings: Vec::new(),
        edges: Vec::new(),
    }
}

fn malformed_native_entry() -> MatrixEntry {
    let digest = format!("b3-{}", "c".repeat(64));
    MatrixEntry::derive(
        "native",
        "internal/native-check/tap_tree",
        "native-homebrew-check-v1",
        &digest,
        serde_json::json!({"incomplete": true}),
        ExecuteTaskIds {
            tasks: BTreeMap::from([(
                "native_check".to_owned(),
                ExecuteTaskRef::Single("internal/native-check/tap_tree".to_owned()),
            )]),
        },
        &digest,
        "local",
        "check-tap-tree",
        PlannedPlatform::new("ubuntu-26.04", "x86_64-unknown-linux-gnu").expect("planned platform"),
    )
    .expect("raw native matrix entry")
}

#[test]
fn fork_and_merge_group_absence_skip_the_full_admission() {
    for event in [WorkflowEvent::Fork, WorkflowEvent::MergeGroup] {
        let plan = plan(event, Vec::new());
        let absence_calls = Cell::new(0);
        let full_calls = Cell::new(0);
        let result = route_required_admission(
            &plan,
            || {
                absence_calls.set(absence_calls.get() + 1);
                Ok(true)
            },
            || {
                full_calls.set(full_calls.get() + 1);
                Err(crate::internal::internal("unexpected_full_admission"))
            },
        );
        assert!(result.is_ok());
        assert_eq!(absence_calls.get(), 1);
        assert_eq!(full_calls.get(), 0);
    }
}

#[test]
fn present_locator_runs_full_admission_and_propagates_its_failure() {
    let plan = plan(WorkflowEvent::PullRequest, Vec::new());
    let full_calls = Cell::new(0);
    let result = route_required_admission(
        &plan,
        || Ok(false),
        || {
            full_calls.set(full_calls.get() + 1);
            Err(crate::internal::internal("full_admission_rejected"))
        },
    );
    assert!(result.is_err());
    assert_eq!(full_calls.get(), 1);
}

#[test]
fn raw_native_entry_with_malformed_metadata_never_uses_the_absence_shortcut() {
    let entry = malformed_native_entry();
    let entries = vec![entry];
    assert!(actual_native_checks(&entries).is_empty());
    assert!(require_native_authority(&entries).is_err());
    let plan = plan(WorkflowEvent::PullRequest, entries);
    let absence_calls = Cell::new(0);
    let full_calls = Cell::new(0);
    let result = route_required_admission(
        &plan,
        || {
            absence_calls.set(absence_calls.get() + 1);
            Ok(true)
        },
        || {
            full_calls.set(full_calls.get() + 1);
            Err(crate::internal::internal("full_admission_rejected"))
        },
    );
    assert!(result.is_err());
    assert_eq!(absence_calls.get(), 0);
    assert_eq!(full_calls.get(), 1);
}

#[test]
fn locator_read_error_propagates_without_falling_back_or_accepting() {
    let plan = plan(WorkflowEvent::Push, Vec::new());
    let full_calls = Cell::new(0);
    let result = route_required_admission(
        &plan,
        || Err(crate::internal::internal("locator_read_failed")),
        || {
            full_calls.set(full_calls.get() + 1);
            Ok(())
        },
    );
    assert!(result.is_err());
    assert_eq!(full_calls.get(), 0);
}

#[test]
fn missing_protected_native_matrix_entry_fails_the_required_validator() {
    let required = NativeHomebrewCheck {
        id: "tap_tree".to_owned(),
        operation: NativeHomebrewOperation::TapTree,
        formula_channel: None,
        runner: CheckRunner {
            label: "ubuntu-26.04".to_owned(),
            platform: CheckPlatform::LinuxX64,
            executor: CheckExecutor::Hosted,
            container: None,
        },
        timeout_minutes: 5,
    };
    assert!(validate_required_matrix_entry(&BTreeMap::new(), &required).is_err());
}
