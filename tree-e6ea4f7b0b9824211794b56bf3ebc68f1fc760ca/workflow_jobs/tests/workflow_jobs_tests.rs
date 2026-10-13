use super::*;

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
fn plan_tools_follow_role_in_all_order() {
    assert_eq!(
        plan_tools(true, false, false),
        vec![
            PinnedTool::Rust,
            PinnedTool::Actionlint,
            PinnedTool::Shellcheck,
            PinnedTool::Zizmor,
        ]
    );
    assert_eq!(
        plan_tools(false, false, true),
        vec![
            PinnedTool::Actionlint,
            PinnedTool::Shellcheck,
            PinnedTool::Zizmor,
            PinnedTool::Opentofu,
        ],
        "pure-tofu plans carry opentofu plus the validators, no Rust"
    );
    assert_eq!(
        plan_tools(true, true, true),
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
        plan_tools(true, false, false),
        plan_tools(false, false, true),
        plan_tools(true, true, true),
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
            "native action owns MBX installation even when selected: use_mbx={use_mbx}; {run:?}"
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
fn pure_tofu_plan_drops_all_rust_setup() {
    use velnor_actions_mise::PREPARE_RUST_COMPONENTS_STEP;

    use crate::source_prep::FETCH_SOURCES_STEP;
    let catalog = ToolCatalog::pinned();
    let job = plan_job(
        "ubuntu-26.04",
        None,
        &catalog,
        false,
        false,
        false,
        true,
        &[],
    )
    .expect("plan job");
    let names: Vec<&str> = job.steps.iter().map(|step| step.name.as_str()).collect();
    assert!(
        !names.contains(&PREPARE_RUST_COMPONENTS_STEP),
        "no components step: {names:?}"
    );
    assert!(
        !names
            .iter()
            .any(|name| name.starts_with(FETCH_SOURCES_STEP)),
        "no {FETCH_SOURCES_STEP}: {names:?}"
    );
    let prepare_at = names
        .iter()
        .position(|name| *name == PREPARE_PINNED_TOOLS_STEP)
        .expect("prepare step");
    let StepKind::Shell { run, env } = &job.steps[prepare_at].kind else {
        panic!("prepare must be a shell step: {names:?}");
    };
    let install_at = run
        .iter()
        .position(|arg| arg == "install")
        .expect("install argv");
    let specs = &run[install_at + 1..];
    assert_eq!(
        specs,
        [
            catalog.tool_spec(PinnedTool::Actionlint),
            catalog.tool_spec(PinnedTool::Shellcheck),
            catalog.tool_spec(PinnedTool::Zizmor),
            catalog.tool_spec(PinnedTool::Opentofu),
        ]
        .as_slice(),
        "pure-tofu plan installs opentofu plus validators: {run:?}"
    );
    for key in ["MISE_RUSTUP_HOME", "MISE_CARGO_HOME", "RUSTUP_TOOLCHAIN"] {
        assert!(!env.contains_key(key), "prepare carries no {key}: {env:?}");
    }
    assert_eq!(
        env.get("MISE_NO_CONFIG").map(String::as_str),
        Some("1"),
        "isolation overlay stays: {env:?}"
    );
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
