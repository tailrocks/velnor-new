//! Fail-closed controls for phased tool transports.

use super::*;

fn setup_step_at(job: &Job) -> usize {
    job.steps
        .iter()
        .position(|step| {
            matches!(&step.kind, StepKind::SourceBoundHelper { invocation, .. }
                if invocation.descriptor().operation()
                    == velnor_actions_contract::SourceBoundOperation::MiseBootstrap)
        })
        .expect("compiled Mise setup")
}

fn mutate_path(job: &mut Job, name: &str, path: &str) {
    let at = at_name(job, name);
    let StepKind::Action { with, .. } = &mut job.steps[at].kind else {
        panic!("cache action")
    };
    with.insert("path".to_owned(), path.to_owned());
}

fn forged_save(mut job: Job, name: &str, path: &str) -> Job {
    let mut save = crate::cache_steps::cache_action_step(
        false,
        crate::cache_steps::TOOLS_SAVE_USES,
        "tools",
        "forged-owned-save",
        &[],
        &[path.to_owned()],
    )
    .expect("forged save");
    save.name = name.to_owned();
    job.steps.push(save);
    job
}

fn direct_mise_step(name: &str, path: &str, root: &str, condition: Option<&str>) -> Step {
    let mut step = crate::steps::ambient_shell_step(
        name,
        vec![path.to_owned(), "--version".to_owned()],
        BTreeMap::from([("MISE_DATA_DIR".to_owned(), root.to_owned())]),
    )
    .expect("direct Mise step");
    step.condition = condition.map(str::to_owned);
    step
}

fn wrong_owner_helper() -> Step {
    let operation = velnor_actions_contract::SourceBoundOperation::MiseToolPrepare;
    let descriptor = velnor_actions_contract::SourceBoundHelper::compiled(
        operation,
        operation.path(),
        &"ab".repeat(32),
    )
    .expect("descriptor");
    let invocation = velnor_actions_contract::HelperInvocation::compiled(
        descriptor,
        vec!["gh".to_owned()],
        vec!["gh@2.102.0".to_owned()],
    )
    .expect("invocation");
    let source = velnor_actions_contract::generated_source("0.1.0", "exit 0\n").expect("source");
    let environment = std::collections::BTreeMap::from([(
        "MISE_DATA_DIR".to_owned(),
        velnor_actions_contract::ToolCacheDomain::Full
            .root()
            .to_owned(),
    )]);
    let record = velnor_actions_contract::CompiledSourceHelper::compiled(invocation, source)
        .expect("owner record")
        .with_environment(environment.clone());
    crate::source_helper::source_helper_step("Wrong owner root", &record, environment)
        .expect("wrong-root helper")
}

#[test]
fn duplicate_renamed_bootstrap_is_rejected() {
    let mut job = rendered();
    let mut duplicate = job.steps[setup_step_at(&job)].clone();
    duplicate.name = "Renamed duplicate bootstrap".to_owned();
    job.steps.push(duplicate);
    assert!(ensure("duplicate-setup", &mut job, &setup(), TARGET).is_err());
}

#[test]
fn qualified_bootstrap_pin_substitution_is_rejected() {
    let mut job = rendered();
    let at = setup_step_at(&job);
    let StepKind::SourceBoundHelper { env, .. } = &mut job.steps[at].kind else {
        panic!("compiled bootstrap")
    };
    env.insert("VELNOR_MISE_SHA256".to_owned(), "b".repeat(64));
    assert!(ensure("changed-bootstrap", &mut job, &setup(), TARGET).is_err());
}

#[test]
fn consumer_rejects_renamed_owned_save_and_losing_writer() {
    let jobs = BTreeMap::from([
        ("rust-a".to_owned(), rendered()),
        ("rust-b".to_owned(), rendered()),
    ]);
    crate::cache_p08::validate_tool_consumers(&jobs, &setup(), &[])
        .expect("shared readonly consumers");

    let mut forged = BTreeMap::from([
        ("rust-a".to_owned(), rendered()),
        (
            "rust-b".to_owned(),
            forged_save(
                rendered(),
                "Renamed owned save",
                ToolCacheDomain::Full.root(),
            ),
        ),
    ]);
    let error = crate::cache_p08::validate_tool_consumers(&forged, &setup(), &[])
        .expect_err("consumer writer must reject");
    assert!(error.to_string().contains("tool_consumer_write"));
    forged.remove("rust-a");
    assert!(crate::cache_p08::validate_tool_consumers(&forged, &setup(), &[]).is_err());
}

#[test]
fn duplicate_early_and_stable_plan_operations_are_rejected() {
    let mut early = fixture();
    early
        .steps
        .push(crate::early_plan::early_plan_step().expect("duplicate early plan"));
    assert!(ensure("duplicate-early", &mut early, &setup(), TARGET).is_err());

    let mut stable = fixture();
    stable.steps.push(crate::steps::plan_step());
    assert!(ensure("duplicate-stable", &mut stable, &setup(), TARGET).is_err());
}

#[test]
fn first_render_rejects_wrong_owner_helper_root() {
    let mut job = fixture();
    job.steps.insert(1, wrong_owner_helper());
    assert!(ensure("wrong-owner-root", &mut job, &setup(), TARGET).is_err());
}

#[test]
fn canonical_planner_bindings_and_preflight_order_are_rejected_when_changed() {
    let mut early = rendered();
    let early_at = at_id(&early, crate::early_plan::EARLY_PLAN_STEP_ID);
    early.steps[early_at].condition = Some("true".to_owned());
    assert!(ensure("changed-early", &mut early, &setup(), TARGET).is_err());

    let mut plan = rendered();
    let plan_at = plan
        .steps
        .iter()
        .position(|step| {
            matches!(&step.kind, StepKind::Internal { operation }
                if operation == crate::steps::PLAN_OPERATION)
        })
        .expect("stable plan");
    plan.steps[plan_at].condition = Some("true".to_owned());
    assert!(ensure("changed-plan", &mut plan, &setup(), TARGET).is_err());

    let mut misplaced = rendered();
    let preflight_at = misplaced
        .steps
        .iter()
        .position(|step| step.name == "Verify restored Mise binary")
        .expect("full preflight");
    let stable_plan_at = misplaced
        .steps
        .iter()
        .position(|step| {
            matches!(&step.kind, StepKind::Internal { operation }
                if operation == crate::steps::PLAN_OPERATION)
        })
        .expect("stable plan");
    misplaced.steps.swap(preflight_at, stable_plan_at);
    assert!(ensure("preflight-after-plan", &mut misplaced, &setup(), TARGET).is_err());
}

#[test]
fn full_prepare_requires_the_cargo_fallback_guard() {
    let mut missing = rendered();
    let prepare = at_name(&missing, "Prepare Rust");
    missing.steps[prepare].condition = None;
    assert!(ensure("missing-full-guard", &mut missing, &setup(), TARGET).is_err());

    let mut unconditional = rendered();
    let prepare = at_name(&unconditional, "Prepare Rust");
    unconditional.steps[prepare].condition = Some("always()".to_owned());
    assert!(
        ensure(
            "unconditional-full-use",
            &mut unconditional,
            &setup(),
            TARGET
        )
        .is_err()
    );
}

#[test]
fn after_early_cargo_paths_require_the_cargo_fallback_guard() {
    let mut direct = rendered();
    direct.steps.push(
        crate::steps::ambient_shell_step(
            "Direct Cargo path",
            vec!["$CARGO_HOME/bin/cargo".to_owned(), "--version".to_owned()],
            BTreeMap::new(),
        )
        .expect("direct cargo step"),
    );
    assert!(ensure("direct-cargo", &mut direct, &setup(), TARGET).is_err());

    let mut shell = rendered();
    shell.steps.push(
        crate::steps::ambient_shell_step(
            "Shell Cargo path",
            vec![
                "sh".to_owned(),
                "-c".to_owned(),
                "$CARGO_HOME/bin/cargo --version".to_owned(),
            ],
            BTreeMap::new(),
        )
        .expect("shell cargo step"),
    );
    assert!(ensure("shell-cargo", &mut shell, &setup(), TARGET).is_err());
}

#[test]
fn ordinary_direct_mise_path_cannot_bypass_acquisition() {
    let mut job = fixture();
    job.steps = vec![
        crate::steps::ambient_shell_step(
            "Direct Mise path",
            vec![
                "$RUNNER_TEMP/velnor/mise/bin/mise".to_owned(),
                "--version".to_owned(),
            ],
            BTreeMap::new(),
        )
        .expect("direct Mise path"),
    ];
    let result = crate::cache_p08::ensure_setup_p08(
        "ordinary-direct-mise",
        &mut job,
        &setup(),
        false,
        TARGET,
        &[],
    );
    if result.is_ok() {
        let setup_at = setup_step_at(&job);
        let direct_at = at_name(&job, "Direct Mise path");
        assert!(setup_at < direct_at, "acquisition must precede direct Mise");
    }
}

#[test]
fn guarded_full_direct_mise_before_restore_is_rejected() {
    let mut job = rendered();
    let full_restore = at_id(&job, crate::cache_steps::TOOLS_RESTORE_ID);
    let root = ToolCacheDomain::Full.root();
    job.steps.insert(
        full_restore,
        direct_mise_step(
            "Premature direct Full Mise",
            &format!("{root}/bin/mise"),
            root,
            Some(crate::early_plan::NEEDS_CARGO_CONDITION),
        ),
    );
    assert!(ensure("premature-direct-full", &mut job, &setup(), TARGET).is_err());
}

#[test]
fn full_direct_mise_before_early_cannot_use_planning_root() {
    let mut job = fixture();
    let full_root = ToolCacheDomain::Full.root();
    job.steps.insert(
        1,
        direct_mise_step(
            "Full Mise before early plan",
            &format!("{full_root}/bin/mise"),
            PLANNING_ROOT,
            None,
        ),
    );
    assert!(ensure("full-before-early", &mut job, &setup(), TARGET).is_err());
}

#[test]
fn ordinary_planning_mise_path_rejects_full_acquisition() {
    let mut job = fixture();
    let root = ToolCacheDomain::Planning.root();
    job.steps = vec![direct_mise_step(
        "Direct Planning Mise",
        &format!("{root}/bin/mise"),
        root,
        None,
    )];
    assert!(
        crate::cache_p08::ensure_setup_p08(
            "ordinary-planning-mise",
            &mut job,
            &setup(),
            false,
            TARGET,
            &[]
        )
        .is_err()
    );
}

#[test]
fn root_domain_use_and_order_tampering_are_rejected() {
    let mut root = rendered();
    let at = at_name(&root, "Install planning tools");
    let StepKind::Shell { env, .. } = &mut root.steps[at].kind else {
        panic!("planning shell")
    };
    env.insert(
        "MISE_DATA_DIR".to_owned(),
        crate::cache_steps::TOOLS_CACHE_PATH.to_owned(),
    );
    assert!(ensure("planning-root", &mut root, &setup(), TARGET).is_err());

    let mut domain = rendered();
    let at = domain
        .steps
        .iter()
        .position(|step| {
            step.name == "Verify restored Mise binary"
                && shell_env(step)
                    .get("VELNOR_MISE_CACHE_DOMAIN")
                    .map(String::as_str)
                    == Some("tools")
        })
        .expect("full preflight");
    let StepKind::Shell { env, .. } = &mut domain.steps[at].kind else {
        panic!("full preflight")
    };
    env.insert("VELNOR_MISE_CACHE_DOMAIN".to_owned(), "planning".to_owned());
    assert!(ensure("preflight-domain", &mut domain, &setup(), TARGET).is_err());

    let mut early_use = rendered();
    let full_at = at_id(&early_use, crate::cache_steps::TOOLS_RESTORE_ID);
    let use_step = crate::steps::ambient_shell_step(
        "Premature full tool use",
        vec![
            "mise".to_owned(),
            "exec".to_owned(),
            "rust@1.98.1".to_owned(),
            "--".to_owned(),
            "rustc".to_owned(),
            "--version".to_owned(),
        ],
        BTreeMap::new(),
    )
    .expect("premature use");
    early_use.steps.insert(full_at, use_step);
    assert!(ensure("early-use", &mut early_use, &setup(), TARGET).is_err());

    let mut order = rendered();
    let full_at = at_id(&order, crate::cache_steps::TOOLS_RESTORE_ID);
    let early_at = at_id(&order, crate::early_plan::EARLY_PLAN_STEP_ID);
    order.steps.swap(full_at, early_at);
    assert!(ensure("wrong-order", &mut order, &setup(), TARGET).is_err());
}

#[test]
fn restore_paths_reject_aliases_and_parent_roots() {
    let paths = [
        "$RUNNER_TEMP/velnor/planning/mise",
        "${{ runner.temp }}/velnor/planning/mise/",
        "${{ runner.temp }}/velnor/planning/./mise",
        "${{ runner.temp }}/velnor/planning/mise/..",
        "${{ runner.temp }}/velnor/planning",
    ];
    for path in paths {
        let mut restore = rendered();
        mutate_path(&mut restore, PLANNING_RESTORE_NAME, path);
        assert!(
            ensure("path-restore", &mut restore, &setup(), TARGET).is_err(),
            "accepted restore path {path}"
        );
    }
}
