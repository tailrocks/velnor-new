use std::collections::BTreeMap;
use std::path::Path;

use velnor_actions_contract_planning::ProposedTask;
use velnor_actions_contract_workflow::WorkflowEvent;
use velnor_actions_mise::ToolCatalog;
use velnor_actions_orchestrator_discovery::discover::Discovery;
use velnor_actions_orchestrator_graph::internal_plan::snapshot::ExecutionSnapshot;
use velnor_actions_orchestrator_graph::internal_plan::wire_w2::GroupWire;
use velnor_actions_orchestrator_plan_obligation::plan_obligation::{GroupInputs, plan_group};

/// Runner label carried by every planned entry.
pub(super) const LABEL: &str = "ubuntu-26.04";

/// Minimal discovery with explicit proposals.
pub(super) fn discovery_with(proposals: Vec<ProposedTask>) -> Discovery {
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

/// Tofu proposal via the T12 adapter constructor.
pub(super) fn tofu_proposal(kind: velnor_actions_tofu_core::TofuTaskKind) -> ProposedTask {
    let group = velnor_actions_tofu_core::TofuTaskGroup {
        root: String::new(),
        kind,
        configuration: "default".to_owned(),
        no_targets: false,
    };
    let task = velnor_actions_tofu_core::propose_task(&group).expect("fixture proposes");
    task.validate().expect("fixture valid");
    task
}

/// Rust clippy proposal via the adapter constructor.
pub(super) fn rust_proposal() -> ProposedTask {
    use velnor_actions_rust::{TaskGroup, TaskKind};
    use velnor_actions_rust_core::{CompileDriver, NextestProfile, TestRunner};
    let group = TaskGroup {
        task_id: "stack/rust/root/clippy/default".to_owned(),
        package_id: "demo".to_owned(),
        package_name: "demo".to_owned(),
        manifest_key: "root".to_owned(),
        kind: TaskKind::Clippy,
        configuration: "default".to_owned(),
        features: Vec::new(),
        target: "host".to_owned(),
        gated_by: Vec::new(),
        depends_on: Vec::new(),
        target_flags: Vec::new(),
        no_test_targets: false,
        package_arg: None,
        compile_driver: CompileDriver::Cargo,
        test_runner: TestRunner::CargoTest,
        nextest_profile: NextestProfile::Default,
        declared_inputs: Vec::new(),
        undeclared_reads: false,
        uses_network: false,
        uses_clock: false,
        uses_random: false,
        run_ignored: None,
    };
    let task = velnor_actions_rust::propose_task(&group).expect("fixture proposes");
    task.validate().expect("fixture valid");
    task
}

/// Plan one task against a discovery carrying exactly that task.
#[expect(
    clippy::too_many_arguments,
    reason = "one plan call threads the full GroupInputs fixture"
)]
pub(super) fn planned(
    task: &ProposedTask,
    discovery: &Discovery,
    snapshot: &ExecutionSnapshot,
    catalog: &ToolCatalog,
    generator: &velnor_actions_contract_workflow::PlanGenerator,
    lanes: &BTreeMap<String, Vec<velnor_actions_contract_workflow::NamedCheckLane>>,
    root: &Path,
    changed: bool,
) -> (
    velnor_actions_contract_workflow::PlanObligation,
    Vec<velnor_actions_contract_workflow::MatrixEntry>,
) {
    let wire = GroupWire {
        event: WorkflowEvent::Push,
        generator,
    };
    let inputs = GroupInputs {
        discovery,
        task,
        run_key: "r7-a1",
        label: LABEL,
        lane: 0,
        catalog,
        wire,
        changed,
        snapshot,
        root,
        named_check_lanes: lanes,
    };
    plan_group(&inputs, &mut velnor_actions_tofu_core::FileCache::new()).expect("plans")
}

/// Root carrying a tofu lockfile for closure-bound identities.
pub(super) fn tofu_root() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join(".terraform.lock.hcl"), "lock").expect("lockfile");
    dir
}
