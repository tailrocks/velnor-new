//! Workflow job constructor tests.
//!
//! Declared via `#[path]` from `workflow_jobs.rs` under `cfg(test)`.

use super::*;
use velnor_actions_contract::StepKind;
use velnor_actions_mise::PREPARE_PINNED_TOOLS_STEP;

/// Internal operation of one step, if any.
fn operation_of(step: &Step) -> Option<&str> {
    match &step.kind {
        StepKind::Internal { operation } => Some(operation),
        StepKind::Action { .. } | StepKind::Shell { .. } | StepKind::SourceBoundHelper { .. } => {
            None
        }
    }
}

/// Assert Write request precedes `target` with the expected operations.
fn assert_request_before(job: &Job, target: &str, request: &str, operation: &str) {
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
            true,
            false,
            false,
            false,
            &[],
        )
        .expect("plan job");
        assert_request_before(&job, "Plan", "write-request-v1:plan-v1", PLAN_OPERATION);
    }
}

#[test]
fn plan_job_checks_out_full_history_for_archaeology() {
    let catalog = ToolCatalog::pinned();
    let job = plan_job(
        "ubuntu-26.04",
        None,
        &catalog,
        true,
        false,
        false,
        false,
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
fn consumer_attempts_planning_before_every_rust_preparation_step() {
    let job = plan_job(
        "ubuntu-26.04",
        Some(checkout_action().expect("acquire position fixture")),
        &ToolCatalog::pinned(),
        true,
        true,
        true,
        false,
        &[String::new()],
    )
    .expect("consumer plan");
    let early_at = job
        .steps
        .iter()
        .position(|step| {
            operation_of(step)
                == Some(velnor_actions_workflow_renderer::steps::EARLY_PLAN_OPERATION)
        })
        .expect("early attempt");
    let plan_at = job
        .steps
        .iter()
        .position(|step| operation_of(step) == Some(PLAN_OPERATION))
        .expect("stable plan");
    let small = job
        .steps
        .iter()
        .find(|step| step.name == "Prepare planning tools")
        .expect("small planning tools");
    let StepKind::Shell { run, env } = &small.kind else {
        panic!("planning shell")
    };
    assert!(!env.contains_key("RUSTUP_HOME"));
    assert!(
        run.iter()
            .all(|word| !word.contains("rust@") && !word.contains("mr-boxington@"))
    );
    assert!(early_at < plan_at);
    for step in &job.steps[early_at + 1..plan_at] {
        assert!(
            step.condition.as_ref().is_some_and(|condition| {
                condition
                    .contains(velnor_actions_workflow_renderer::early_plan::NEEDS_CARGO_CONDITION)
            }),
            "fallback preparation must require Cargo: {}",
            step.name
        );
    }
    assert!(
        job.steps[plan_at].condition.is_none(),
        "stable outputs always materialize"
    );
}

#[test]
fn plan_tools_follow_role_in_all_order() {
    assert_eq!(
        plan_tools(true, false, false, false),
        vec![
            PinnedTool::Rust,
            PinnedTool::Actionlint,
            PinnedTool::Shellcheck,
            PinnedTool::Zizmor,
        ]
    );
    assert_eq!(
        plan_tools(false, false, false, true),
        vec![
            PinnedTool::Actionlint,
            PinnedTool::Shellcheck,
            PinnedTool::Zizmor,
            PinnedTool::Opentofu,
        ],
        "pure-tofu plans carry opentofu plus the validators, no Rust"
    );
    assert_eq!(
        plan_tools(true, true, true, true),
        vec![
            PinnedTool::Rust,
            PinnedTool::MrBoxington,
            PinnedTool::Actionlint,
            PinnedTool::Shellcheck,
            PinnedTool::Zizmor,
            PinnedTool::Nextest,
            PinnedTool::Opentofu,
        ],
        "mixed plans carry the union"
    );
    for tools in [
        plan_tools(true, false, false, false),
        plan_tools(false, false, false, true),
        plan_tools(true, true, true, true),
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
    for (use_mbx, use_nextest) in [(false, false), (false, true), (true, true)] {
        let job = plan_job(
            "ubuntu-26.04",
            None,
            &catalog,
            true,
            use_mbx,
            use_nextest,
            false,
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
        let StepKind::SourceBoundHelper { invocation, env } =
            &job.steps[prepare_at.expect("prepare step")].kind
        else {
            panic!("Rust prepare must bind compiled source: {names:?}");
        };
        let owner = velnor_actions_mise::catalog::rust_prepare::record_for_invocation(
            invocation,
            env,
            env!("CARGO_PKG_VERSION"),
        )
        .expect("exact preparation owner environment");
        let install = velnor_actions_mise::catalog::rust_prepare::install_argv(
            owner.invocation(),
            env!("CARGO_PKG_VERSION"),
        )
        .expect("owner-bound Rust installation");
        let run = &install;
        assert_eq!(run[0], "mise");
        let install_at = run.iter().position(|arg| arg == "install");
        let specs = expected_plan_install_specs(&catalog, use_mbx, use_nextest);
        assert_eq!(
            install_at.map(|at| &run[at + 1..]),
            Some(specs.as_slice()),
            "install specs: {run:?}"
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

#[test]
fn final_job_writes_request_before_merge() {
    let catalog = ToolCatalog::pinned();
    for acquire in [None, Some(checkout_action().expect("checkout step"))] {
        let job = final_job("ubuntu-26.04", &["rust-demo".to_owned()], acquire, &catalog)
            .expect("final job");
        assert_request_before(
            &job,
            "Merge reports",
            "write-request-v1:merge-v1",
            MERGE_OPERATION,
        );
    }
}

#[test]
fn final_job_needs_plan_crates_and_lint() {
    let catalog = ToolCatalog::pinned();
    let job = final_job(
        "ubuntu-26.04",
        &["rust-demo".to_owned(), "rust-nested".to_owned()],
        None,
        &catalog,
    )
    .expect("final job");
    assert_eq!(
        job.needs,
        [
            PLAN_JOB_ID.to_owned(),
            "rust-demo".to_owned(),
            "rust-nested".to_owned(),
            LINT_JOB_ID.to_owned(),
        ]
    );
    let job = final_job("ubuntu-26.04", &[], None, &catalog).expect("final job");
    assert_eq!(job.needs, [PLAN_JOB_ID.to_owned(), LINT_JOB_ID.to_owned()]);
}

fn expected_plan_install_specs(
    catalog: &ToolCatalog,
    use_mbx: bool,
    use_nextest: bool,
) -> Vec<String> {
    let mut specs = vec![
        catalog
            .tool_spec(PinnedTool::Rust)
            .expect("qualified selector"),
        catalog
            .tool_spec(PinnedTool::Actionlint)
            .expect("qualified selector"),
        catalog
            .tool_spec(PinnedTool::Shellcheck)
            .expect("qualified selector"),
        catalog
            .tool_spec(PinnedTool::Zizmor)
            .expect("qualified selector"),
    ];
    if use_mbx {
        specs.insert(
            1,
            catalog
                .tool_spec(PinnedTool::MrBoxington)
                .expect("qualified selector"),
        );
    }
    if use_nextest {
        specs.push(
            catalog
                .tool_spec(PinnedTool::Nextest)
                .expect("qualified selector"),
        );
    }
    specs
}
