// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::ffi::OsString;
use velnor_actions_contract::{CachePolicy, IdentityInputs, ProposedTask, ResourceClass, ResourceDemand, StepKind, WorkflowPolicy};
use velnor_actions_mise::ToolCatalog;
use super::{LaneTools, prepare_crate_tools_step};

fn fixture_task(stack: &str, driver: &str, runner: &str) -> ProposedTask {
    ProposedTask {
        task_id: "internal/build/dev".to_owned(),
        stack_id: stack.to_owned(),
        component_id: "demo".to_owned(),
        task_kind: "build".to_owned(),
        configuration: "default".to_owned(),
        depends_on: Vec::new(),
        gated_by: Vec::new(),
        reads: Vec::new(),
        writes: Vec::new(),
        outputs: Vec::new(),
        resource: ResourceDemand {
            class: ResourceClass::Compiler,
            cpu_milli: None,
            memory_mb: None,
            needs_network: false,
            service: None,
        },
        cache_policy: CachePolicy {
            allow_compilation_reuse: false,
            allow_task_reuse: false,
        },
        identity: IdentityInputs {
            unit_id: "demo".to_owned(),
            unit_key: "demo".to_owned(),
            unit_path: "crates/demo/Cargo.toml".to_owned(),
            project_root: String::new(),
            target: String::new(),
            features: Vec::new(),
            flags: Vec::new(),
            compile_driver: driver.to_owned(),
            test_runner: runner.to_owned(),
            environment: BTreeMap::new(),
            declared_inputs: Vec::new(),
            undeclared_reads: false,
        },
        payload: vec![OsString::from("mbx")],
        display_name: "demo".to_owned(),
        uses_clock: false,
        uses_random: false,
        no_targets: false,
        runner_profile: "default".to_owned(),
    }
}


#[test]
fn gh_test_tool_is_provisioned_only_for_velnor() {
    let mut task = fixture_task("rust", "cargo", "cargo_nextest");
    let catalog = ToolCatalog::pinned();
    let has_gh = |policy, task: &ProposedTask| {
        let lane = LaneTools::of_group(policy, &catalog, &[task]);
        let step = prepare_crate_tools_step(&catalog, &lane).expect("render tool installation");
        let StepKind::Shell { run, .. } = step.kind else { panic!("expected shell step") };
        run.iter().any(|arg| arg.contains("gh@2.103.3"))
    };
    for package in ["velnor-actions-orchestrator", "velnor-actions-workflow-renderer"] {
        task.display_name = package.to_owned();
        assert!(has_gh(WorkflowPolicy::VelnorRepositoryV1, &task));
        assert!(!has_gh(WorkflowPolicy::ConsumerV1, &task));
    }
}
