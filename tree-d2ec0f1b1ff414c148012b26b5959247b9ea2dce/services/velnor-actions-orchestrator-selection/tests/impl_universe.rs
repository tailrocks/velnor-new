//! Universe filtering and task-change classification.

use std::collections::BTreeSet;

use velnor_actions_contract_planning::ProposedTask;
use velnor_actions_orchestrator_discovery::discover::Discovery;
use velnor_actions_orchestrator_selection::select::{group_changed, select_universe};

/// Empty discovery carrying exactly `proposals`.
fn discovery_with(proposals: Vec<ProposedTask>) -> Discovery {
    Discovery {
        mise_checks: Vec::new(),
        statuses: Vec::new(),
        workspaces: Vec::new(),
        proposals,
        feature_fallbacks: Vec::new(),
        tool_checks: Vec::new(),
        clippy_memory: velnor_actions_orchestrator_core::clippy_groups::ClippyMemoryPlan {
            groups: Vec::new(),
            barriers: 0,
        },
        recommendations: Vec::new(),
        consumer_manifest_json: None,
        skipped_non_utf8: false,
        tofu_note: None,
        tofu_units: Vec::new(),
    }
}

/// Minimal clippy proposal for change fixtures.
fn proposal() -> ProposedTask {
    let group = velnor_actions_rust::TaskGroup {
        task_id: "stack/rust/root/clippy/default".to_owned(),
        package_id: String::new(),
        package_name: String::new(),
        manifest_key: "root".to_owned(),
        kind: velnor_actions_rust::TaskKind::Clippy,
        configuration: "default".to_owned(),
        features: Vec::new(),
        target: "host".to_owned(),
        gated_by: Vec::new(),
        depends_on: Vec::new(),
        target_flags: Vec::new(),
        no_test_targets: false,
        package_arg: None,
        compile_driver: velnor_actions_rust_core::CompileDriver::Cargo,
        test_runner: velnor_actions_rust_core::TestRunner::CargoTest,
        declared_inputs: Vec::new(),
        undeclared_reads: false,
        uses_network: false,
        uses_clock: false,
        uses_random: false,
        run_ignored: None,
        nextest_profile: velnor_actions_rust_core::NextestProfile::Default,
    };
    let task = velnor_actions_rust::propose_task(&group).expect("fixture proposes");
    task.validate().expect("fixture valid");
    task
}

#[test]
fn universe_keeps_configured_tasks() {
    let discovery = discovery_with(vec![proposal()]);
    let mut warnings = Vec::new();
    let kept = select_universe(&discovery, &mut warnings);
    assert_eq!(kept.len(), 1);
    assert!(warnings.is_empty());
}

#[test]
fn universe_drops_no_target_tasks_with_warning() {
    let mut task = proposal();
    task.no_targets = true;
    let discovery = discovery_with(vec![task]);
    let mut warnings = Vec::new();
    let kept = select_universe(&discovery, &mut warnings);
    assert!(kept.is_empty());
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].starts_with("valid_no_test_targets:"));
}

#[test]
fn group_changed_matches_unit() {
    let task = proposal();
    assert!(group_changed(
        &task,
        &BTreeSet::from([task.identity.unit_id.clone()]),
        &BTreeSet::new(),
    ));
}

#[test]
fn group_unchanged_without_match() {
    let task = proposal();
    assert!(!group_changed(
        &task,
        &BTreeSet::from(["other".to_owned()]),
        &BTreeSet::new(),
    ));
    assert!(!group_changed(&task, &BTreeSet::new(), &BTreeSet::new(),));
}

#[test]
fn group_changed_matches_key_for_empty_unit() {
    let mut task = proposal();
    task.identity.unit_id.clear();
    assert!(group_changed(
        &task,
        &BTreeSet::new(),
        &BTreeSet::from([task.identity.unit_key.clone()]),
    ));
    assert!(!group_changed(
        &task,
        &BTreeSet::new(),
        &BTreeSet::from(["other".to_owned()]),
    ));
}
