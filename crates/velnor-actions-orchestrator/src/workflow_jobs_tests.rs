//! Workflow job constructor tests.
//!
//! Declared via `#[path]` from `workflow_jobs.rs` under `cfg(test)`.

use super::*;
use crate::clippy_groups::ClippyMemoryPlan;
use crate::discover::Discovery;
use crate::workflow::plan_uses_rust;
use velnor_actions_contract::StepKind;
use velnor_actions_contract::WorkflowPolicy;

/// Discovery with no selected workloads.
fn empty_discovery() -> Discovery {
    Discovery {
        mise_checks: Vec::new(),
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
fn consumer_plan_uses_rust_only_for_selected_rust_evidence() {
    assert!(!plan_uses_rust(
        &empty_discovery(),
        WorkflowPolicy::ConsumerV1
    ));
}

#[test]
fn generator_plan_keeps_rust_for_candidate_helper() {
    assert!(plan_uses_rust(
        &empty_discovery(),
        WorkflowPolicy::VelnorRepositoryV1
    ));
}

#[test]
fn rust_proposals_require_rust_without_inventory_records() {
    let profile = velnor_actions_rust::RustExecutionProfile {
        compile_driver: velnor_actions_rust::CompileDriver::Cargo,
        test_runner: velnor_actions_rust::TestRunner::CargoTest,
        evidence: Vec::new(),
        driver_source: velnor_actions_rust::ProfileSource::Detected,
        runner_source: velnor_actions_rust::ProfileSource::Detected,
        nextest_profile: velnor_actions_rust::NextestProfile::Default,
        nextest_config: None,
        run_ignored: None,
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

pub(crate) fn needs(rust: PlanRustNeed) -> PlanJobToolNeeds {
    PlanJobToolNeeds {
        rust,
        ..PlanJobToolNeeds::default()
    }
}

/// Internal operation of one step, if any.
fn operation_of(step: &Step) -> Option<&str> {
    match &step.kind {
        StepKind::Internal { operation, .. } => Some(operation),
        StepKind::Action { .. } | StepKind::Shell { .. } => None,
    }
}

/// Assert Write request precedes `target` with the expected operations.
pub(crate) fn assert_request_before(job: &Job, target: &str, request: &str, operation: &str) {
    let names: Vec<&str> = job.steps.iter().map(|step| step.name.as_str()).collect();
    let write_at = names.iter().position(|name| *name == "Write request");
    let target_at = names.iter().position(|name| *name == target);
    assert!(
        write_at.is_some_and(|write| Some(write) < target_at),
        "request must precede {target}: {names:?}"
    );
    assert_eq!(
        operation_of(&job.steps[write_at.expect("write request step")]),
        Some(request),
        "request must target {target}"
    );
    assert_eq!(
        operation_of(&job.steps[target_at.expect("target step")]),
        Some(operation)
    );
}

#[test]
fn plan_job_writes_request_before_plan() {
    let catalog = ToolCatalog::pinned();
    for acquire in [None, Some(checkout_action().expect("checkout step"))] {
        let job = plan_job(
            "ubuntu-26.04",
            acquire,
            &catalog,
            needs(PlanRustNeed::CompilerAndComponents),
            &[],
        )
        .expect("plan job");
        assert_request_before(&job, "Plan", "write-request-v1:plan-v1", PLAN_OPERATION);
        assert!(
            !job.steps
                .iter()
                .any(|step| step.name == "Resolve qualification predecessor"),
            "consumer planner has no resolver: {:?}",
            job.steps
        );
    }
}

#[test]
fn qualification_resolver_is_scoped_between_request_and_plan() {
    let catalog = ToolCatalog::pinned();
    let job = plan_job(
        "ubuntu-26.04",
        None,
        &catalog,
        PlanJobToolNeeds {
            gh: true,
            ..needs(PlanRustNeed::CompilerAndComponents)
        },
        &[],
    )
    .expect("qualification plan job");
    let names: Vec<&str> = job.steps.iter().map(|step| step.name.as_str()).collect();
    let request = names
        .iter()
        .position(|name| *name == "Write request")
        .expect("request step");
    let resolver = names
        .iter()
        .position(|name| *name == "Resolve qualification predecessor")
        .expect("resolver step");
    let plan = names
        .iter()
        .position(|name| *name == "Plan")
        .expect("plan step");
    assert!(request < resolver && resolver < plan, "{names:?}");
    assert_eq!(
        job.steps[resolver].condition.as_deref(),
        Some("github.event_name == 'workflow_dispatch'")
    );
    let tools = plan_tools(PlanJobToolNeeds {
        gh: true,
        ..needs(PlanRustNeed::CompilerAndComponents)
    });
    assert!(
        tools.contains(&PinnedTool::Gh),
        "resolver installs pinned gh"
    );
}

#[test]
fn plan_job_checks_out_full_history_for_archaeology() {
    let catalog = ToolCatalog::pinned();
    let job = plan_job(
        "ubuntu-26.04",
        None,
        &catalog,
        needs(PlanRustNeed::CompilerAndComponents),
        &[],
    )
    .expect("plan job");
    let StepKind::Action { with, .. } = &job.steps[0].kind else {
        panic!("plan must start with checkout");
    };
    assert_eq!(
        with.get("fetch-depth").map(String::as_str),
        Some("0"),
        "plan needs history for HEAD^2 + base diff: {with:?}"
    );
    let lint = lint_job("ubuntu-26.04", &catalog).expect("lint job");
    let StepKind::Action { with, .. } = &lint.steps[0].kind else {
        panic!("lint must start with checkout");
    };
    assert!(
        !with.contains_key("fetch-depth"),
        "lint stays shallow: {with:?}"
    );
}

#[test]
fn plan_tools_follow_role_in_all_order() {
    assert_eq!(
        plan_tools(needs(PlanRustNeed::CompilerAndComponents)),
        vec![
            PinnedTool::Rust,
            PinnedTool::Actionlint,
            PinnedTool::Shellcheck,
            PinnedTool::Zizmor,
        ]
    );
    assert_eq!(
        plan_tools(PlanJobToolNeeds {
            opentofu: true,
            ..needs(PlanRustNeed::None)
        }),
        vec![
            PinnedTool::Actionlint,
            PinnedTool::Shellcheck,
            PinnedTool::Zizmor,
            PinnedTool::Opentofu,
        ],
        "pure-tofu plans carry opentofu plus the validators, no Rust"
    );
    assert_eq!(
        plan_tools(PlanJobToolNeeds {
            nextest: true,
            opentofu: true,
            ..needs(PlanRustNeed::CompilerAndComponents)
        }),
        vec![
            PinnedTool::Rust,
            PinnedTool::Actionlint,
            PinnedTool::Shellcheck,
            PinnedTool::Zizmor,
            PinnedTool::Nextest,
            PinnedTool::Opentofu,
        ],
        "mixed plans carry the union"
    );
    for tools in [
        plan_tools(needs(PlanRustNeed::CompilerAndComponents)),
        plan_tools(PlanJobToolNeeds {
            opentofu: true,
            ..needs(PlanRustNeed::None)
        }),
        plan_tools(PlanJobToolNeeds {
            nextest: true,
            opentofu: true,
            ..needs(PlanRustNeed::CompilerAndComponents)
        }),
    ] {
        let order: Vec<usize> = tools
            .iter()
            .map(|tool| {
                PinnedTool::ALL
                    .iter()
                    .position(|known| known == tool)
                    .expect("plan installs catalog tools only")
            })
            .collect();
        let mut sorted = order.clone();
        sorted.sort_unstable();
        assert_eq!(order, sorted, "plan order follows ALL: {tools:?}");
    }
}

#[test]
fn plan_job_prepares_pinned_tools_before_generate_consumers() {
    let catalog = ToolCatalog::pinned();
    for use_nextest in [false, true] {
        let job = plan_job(
            "ubuntu-26.04",
            None,
            &catalog,
            PlanJobToolNeeds {
                nextest: use_nextest,
                ..needs(PlanRustNeed::CompilerAndComponents)
            },
            &[],
        )
        .expect("plan job");
        let names: Vec<&str> = job.steps.iter().map(|step| step.name.as_str()).collect();
        let prepare_at = names
            .iter()
            .position(|name| *name == PREPARE_PINNED_TOOLS_STEP);
        assert_eq!(
            prepare_at,
            Some(1),
            "prepare sits after checkout: {names:?}"
        );
        let write_at = names.iter().position(|name| *name == "Write request");
        let plan_at = names.iter().position(|name| *name == "Plan");
        assert!(
            prepare_at.is_some_and(|prepare| Some(prepare) < write_at && Some(prepare) < plan_at),
            "prepare must precede request and plan: {names:?}"
        );
        let StepKind::Shell { run, env } = &job.steps[prepare_at.expect("prepare step")].kind
        else {
            panic!("prepare must be a shell step: {names:?}");
        };
        assert_eq!(run[0], "mise");
        let install_at = run.iter().position(|arg| arg == "install");
        let mut specs = vec![
            catalog.tool_spec(PinnedTool::Rust),
            catalog.tool_spec(PinnedTool::Actionlint),
            catalog.tool_spec(PinnedTool::Shellcheck),
            catalog.tool_spec(PinnedTool::Zizmor),
        ];
        if use_nextest {
            specs.push(catalog.tool_spec(PinnedTool::Nextest));
        }
        assert_eq!(
            install_at.map(|at| &run[at + 1..]),
            Some(specs.as_slice()),
            "install specs: {run:?}"
        );
        assert!(
            !run.iter().any(|spec| spec.starts_with("mr-boxington@")),
            "native action owns ordinary MBX installation: {run:?}"
        );
        let keys = [
            "MISE_NO_CONFIG",
            "MISE_RUSTUP_HOME",
            "MISE_CARGO_HOME",
            "RUSTUP_TOOLCHAIN",
        ];
        for key in keys {
            assert!(env.contains_key(key), "env misses {key}: {env:?}");
        }
        assert_eq!(
            env.get("MISE_LOCKFILE").map(String::as_str),
            Some("0"),
            "prepare pins the lockfile off so installs never rewrite it: {env:?}"
        );
    }
}
