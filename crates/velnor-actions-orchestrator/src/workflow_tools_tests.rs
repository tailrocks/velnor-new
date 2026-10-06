//! Plan bootstrap tools follow selected workloads and source ownership.

use super::*;
use crate::clippy_groups::ClippyMemoryPlan;
use velnor_actions_mise::{PREPARE_PINNED_TOOLS_STEP, PinnedTool};

fn empty_discovery() -> Discovery {
    Discovery {
        rust_inventory: None,
        raw_inventories: Vec::new(),
        statuses: Vec::new(),
        workspaces: Vec::new(),
        proposals: Vec::new(),
        feature_fallbacks: Vec::new(),
        tool_checks: Vec::new(),
        clippy_memory: ClippyMemoryPlan {
            groups: Vec::new(),
            barriers: 0,
        },
        recommendations: Vec::new(),
        consumer_manifest_json: None,
        consumer_manifest_stand_in: false,
        skipped_non_utf8: false,
        tofu_note: None,
        tofu_units: Vec::new(),
    }
}

#[test]
fn non_rust_consumer_plan_omits_rust_install_and_components() {
    let discovery = empty_discovery();
    let use_rust = plan_uses_rust(&discovery, WorkflowPolicy::ConsumerV1);
    assert!(!use_rust, "a prebuilt helper creates no Rust workload");
    let catalog = ToolCatalog::pinned();
    let plan = build_plan_job(
        "ubuntu-26.04",
        None,
        &catalog,
        use_rust,
        false,
        false,
        false,
        &[],
        &discovery,
    )
    .expect("tools-only plan");
    assert!(
        !plan
            .steps
            .iter()
            .any(|step| step.name == PREPARE_RUST_COMPONENTS_STEP)
    );
    let prepare = plan
        .steps
        .iter()
        .find(|step| step.name == PREPARE_PINNED_TOOLS_STEP)
        .expect("validators remain installed");
    let StepKind::Shell { run, env } = &prepare.kind else {
        panic!("shell prepare")
    };
    assert!(
        !run.contains(
            &catalog
                .tool_spec(PinnedTool::Rust)
                .expect("qualified selector")
        )
    );
    for (key, value) in velnor_actions_contract::ToolCacheDomain::Full.home_environment() {
        assert_eq!(env.get(&key), Some(&value), "Full owns {key}");
    }
    assert!(!env.contains_key("RUSTUP_TOOLCHAIN"));
    assert!(
        !plan
            .steps
            .iter()
            .any(|step| step.name.contains("Cargo") || step.name.contains("MBX"))
    );
}

#[test]
fn generator_plan_retains_current_source_rust_bootstrap() {
    assert!(plan_uses_rust(
        &empty_discovery(),
        WorkflowPolicy::VelnorRepositoryV1
    ));
}

#[test]
fn rust_proposals_require_rust_even_without_inventory_records() {
    let profile = velnor_actions_rust::RustExecutionProfile {
        compile_driver: CompileDriver::Cargo,
        test_runner: TestRunner::CargoTest,
        evidence: Vec::new(),
        driver_source: velnor_actions_rust::ProfileSource::Detected,
        runner_source: velnor_actions_rust::ProfileSource::Detected,
        nextest_profile: velnor_actions_rust::NextestProfile::Default,
        nextest_config: None,
    };
    let group =
        velnor_actions_rust::derive_workspace_fmt("Cargo.toml", &profile, "default", "host")
            .expect("Rust format group");
    let mut discovery = empty_discovery();
    discovery
        .proposals
        .push(velnor_actions_rust::propose_task(&group).expect("Rust proposal"));
    assert!(plan_uses_rust(&discovery, WorkflowPolicy::ConsumerV1));
}

#[test]
fn covered_format_skips_report_staging_and_upload_together() {
    let profile = velnor_actions_rust::RustExecutionProfile {
        compile_driver: CompileDriver::Cargo,
        test_runner: TestRunner::CargoTest,
        evidence: Vec::new(),
        driver_source: velnor_actions_rust::ProfileSource::Detected,
        runner_source: velnor_actions_rust::ProfileSource::Detected,
        nextest_profile: velnor_actions_rust::NextestProfile::Default,
        nextest_config: None,
    };
    let group =
        velnor_actions_rust::derive_workspace_fmt("Cargo.toml", &profile, "default", "host")
            .expect("workspace format group");
    let task = velnor_actions_rust::propose_task(&group).expect("format proposal");
    let expected_condition = format!(
        "always() && !contains(steps.plan.outputs.covered_tasks, ',{},')",
        task.task_id
    );
    let mut discovery = empty_discovery();
    discovery.proposals.push(task);
    let steps = wire_w1::workspace_format_report_steps(&discovery).expect("report steps");
    assert_eq!(steps.len(), 2, "staging immediately precedes upload");
    assert_eq!(steps[0].name, "Stage report payload");
    assert!(matches!(
        &steps[0].kind,
        StepKind::Internal { operation }
            if operation == velnor_actions_workflow_renderer::steps::STAGE_REPORTS_OPERATION
    ));
    assert_eq!(
        steps[1].name,
        velnor_actions_workflow_renderer::CRATE_REPORT_UPLOAD_NAME
    );
    for step in &steps {
        assert_eq!(step.condition.as_deref(), Some(expected_condition.as_str()));
    }
    let StepKind::Action { with, .. } = &steps[1].kind else {
        panic!("report upload must be pinned action");
    };
    assert_eq!(
        with["path"],
        "${{ runner.temp }}/velnor/report-payload/r${{ github.run_id }}-a${{ github.run_attempt }}"
    );
    assert_eq!(with["if-no-files-found"], "error");
}
