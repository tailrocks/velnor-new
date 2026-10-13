use super::*;

#[test]
fn preseed_restores_mbx_builds_after_sources_with_homes() {
    use velnor_actions_contract::StepRole;
    use velnor_actions_workflow_renderer::{PRESEED_BUILD_NAME, PRESEED_VERIFY_NAME};
    let roots = [String::new()];
    let mut plan = preseed_fixture(&roots);
    attach_preseed(&mut plan, "ubuntu-26.04", "0.1.0").expect("attach");
    let steps = &plan.ir.jobs["plan"].steps;
    let names: Vec<&str> = steps.iter().map(|step| step.name.as_str()).collect();
    let at = |name: &str| {
        names
            .iter()
            .position(|step| *step == name)
            .unwrap_or_else(|| panic!("missing {name}: {names:?}"))
    };
    let (restore, preflight, mbx, version_check, probe, build, verify, save) = (
        at(StepRole::CargoSourcesRestore),
        at(StepRole::MbxPreflight),
        at(StepRole::MbxCache),
        at(StepRole::MbxVersionCheck),
        at(StepRole::CargoSourcesFetch),
        at(StepRole::PreseedBuild),
        at(StepRole::PreseedVerifyBuild),
        at(StepRole::CargoSourcesSave),
    );
    assert!(
        at(StepRole::PreparePinnedTools) < restore
            && restore < preflight
            && preflight < mbx
            && mbx < version_check
            && version_check < probe
            && probe < build
            && build < verify
            && verify < save,
        "the native action owns MBX installation after Rust is ready: {names:?}"
    );
    assert!(
        steps.iter().all(|step| !matches!(
            &step.kind,
            velnor_actions_contract::StepKind::Shell { run, .. }
                if run.iter().any(|argument| argument == "mr-boxington@1.24.0")
        )),
        "Mise must not install action-owned MBX: {names:?}"
    );
    assert_owned_homes(steps, StepRole::MbxPreflight, "MBX preflight");
    let mbx_action = steps
        .iter()
        .find(|step| step.name == MBX_RESTORE_NAME)
        .expect("MBX action");
    let velnor_actions_contract::StepKind::Action { env, with, .. } = &mbx_action.kind else {
        panic!("MBX restore must be an action");
    };
    assert_eq!(with.get("toolchain").map(String::as_str), Some("1.99.0"));
    assert!(
        with.get("version").map(String::as_str)
            == Some(ToolCatalog::pinned().version(PinnedTool::MrBoxington)),
        "the native action installs the catalog-pinned MBX version"
    );
    assert_eq!(env.get("RUSTUP_HOME"), env.get("MISE_RUSTUP_HOME"));
    assert_eq!(env.get("CARGO_HOME"), env.get("MISE_CARGO_HOME"));
    assert_owned_homes(
        steps,
        velnor_actions_contract::StepRole::PreseedBuild,
        PRESEED_BUILD_NAME,
    );
    assert_owned_homes(
        steps,
        velnor_actions_contract::StepRole::PreseedVerifyBuild,
        PRESEED_VERIFY_NAME,
    );
}

#[test]
fn lockless_preseed_installs_mbx_before_the_build_without_a_restore() {
    use velnor_actions_workflow_renderer::{
        MBX_PREFLIGHT_NAME, MBX_VERSION_CHECK_NAME, PRESEED_BUILD_NAME,
        cache_p08::RESTORE_SOURCES_NAME,
    };
    let mut plan = preseed_fixture(&[]);
    attach_preseed(&mut plan, "ubuntu-26.04", "0.1.0").expect("attach");
    let steps = &plan.ir.jobs["plan"].steps;
    let names: Vec<&str> = steps.iter().map(|step| step.name.as_str()).collect();
    assert!(!names.contains(&RESTORE_SOURCES_NAME));
    let prepare_components = names
        .iter()
        .position(|step| *step == velnor_actions_mise::PREPARE_RUST_COMPONENTS_STEP)
        .expect("Rust components prepared");
    let preflight = names
        .iter()
        .position(|step| *step == MBX_PREFLIGHT_NAME)
        .expect("preflight");
    let action = names
        .iter()
        .position(|step| *step == MBX_RESTORE_NAME)
        .expect("MBX owner");
    let version_check = names
        .iter()
        .position(|step| *step == MBX_VERSION_CHECK_NAME)
        .expect("MBX version guard");
    let build = names
        .iter()
        .position(|step| *step == PRESEED_BUILD_NAME)
        .expect("preseed build");
    assert!(
        prepare_components < preflight
            && preflight < action
            && action < version_check
            && version_check < build,
        "Rust setup and exact MBX installation precede the build: {names:?}"
    );
    assert_eq!(
        names
            .iter()
            .filter(|step| **step == MBX_RESTORE_NAME)
            .count(),
        1
    );
    assert_owned_homes(
        steps,
        velnor_actions_contract::StepRole::MbxPreflight,
        MBX_PREFLIGHT_NAME,
    );
    assert_owned_homes(
        steps,
        velnor_actions_contract::StepRole::PreseedBuild,
        PRESEED_BUILD_NAME,
    );
}

#[test]
fn preseed_attach_builds_once_and_sets_mode() {
    use velnor_actions_actionlint::ActionlintConfigInput;
    use velnor_actions_workflow_renderer::{
        MBX_VERSION_CHECK_NAME, PRESEED_BUILD_NAME, PRESEED_STAGE_NAME,
    };
    let catalog = ToolCatalog::pinned();
    let mut plan = WorkflowPlan {
        ir: bare_ir(BTreeMap::from([
            (
                "plan".to_owned(),
                plan_job("ubuntu-26.04", None, &catalog, rust_plan_needs(), &[]).expect("plan job"),
            ),
            ("rust-demo".to_owned(), legacy_task_job()),
            (
                "required".to_owned(),
                final_job("ubuntu-26.04", &["rust-demo".to_owned()], None, &catalog)
                    .expect("final job"),
            ),
            (
                "publish-baseline".to_owned(),
                baseline_publish_job("ubuntu-26.04", "main", None).expect("publish job"),
            ),
        ])),
        support: None,
        context: RenderContext {
            generator_version: "0.1.0".to_owned(),
            runs_on: "ubuntu-26.04".to_owned(),
            staged_binary: format!("{STAGED_BINARY_PREFIX}0.1.0"),
            request_dir: REQUEST_DIR.to_owned(),
            checkout_uses: CHECKOUT_USES.to_owned(),
            validator_commands: Vec::new(),
            candidate: None,
            preseed: false,
            verification_tasks: Vec::new(),
            plan_consumer_env: std::collections::BTreeMap::new(),
        },
        actionlint: ActionlintConfigInput::new("0.1.0").with_workflow_path(WORKFLOW_PATH),
    };
    assert!(attach_preseed(&mut plan, "ubuntu-26.04", "0.1.0").is_ok());
    assert!(plan.context.preseed);
    let names: Vec<&str> = plan.ir.jobs["plan"]
        .steps
        .iter()
        .map(|step| step.name.as_str())
        .collect();
    assert_eq!(
        names,
        [
            "Checkout",
            "Prepare pinned tools",
            "Prepare Rust components",
            "Verify Rust before MBX action",
            MBX_RESTORE_NAME,
            MBX_VERSION_CHECK_NAME,
            PRESEED_BUILD_NAME,
            "Verify MBX compile (pre-seed trust-on-review)",
            "Write helper manifest (pre-seed trust-on-review)",
            "Upload helper (pre-seed trust-on-review)",
            PRESEED_STAGE_NAME,
            "Write request",
            "Plan",
        ]
    );
    assert_preseed_consumers(&plan);
    assert!(attach_preseed(&mut plan, "ubuntu-26.04-arm", "0.1.0").is_err());
}

#[test]
fn lockless_preseed_installs_mbx_before_the_build_without_a_restore() {
    use velnor_actions_contract::StepRole;
    use velnor_actions_workflow_renderer::PRESEED_BUILD_NAME;
    let mut plan = preseed_fixture(&[]);
    attach_preseed(&mut plan, "ubuntu-26.04", "0.1.0").expect("attach");
    let steps = &plan.ir.jobs["plan"].steps;
    let names: Vec<&str> = steps.iter().map(|step| step.name.as_str()).collect();
    let roles: Vec<Option<StepRole>> = steps.iter().map(|step| step.role).collect();
    assert!(!roles.contains(&Some(StepRole::CargoSourcesRestore)));
    let at = |role| {
        roles
            .iter()
            .position(|seen| *seen == Some(role))
            .unwrap_or_else(|| panic!("missing {role:?}: {roles:?}"))
    };
    let prepare_components = at(StepRole::PrepareRustComponents);
    let preflight = at(StepRole::MbxPreflight);
    let action = at(StepRole::MbxCache);
    let version_check = at(StepRole::MbxVersionCheck);
    let build = at(StepRole::PreseedBuild);
    assert!(
        prepare_components < preflight
            && preflight < action
            && action < version_check
            && version_check < build,
        "Rust setup and exact MBX installation precede the build: {names:?}"
    );
    assert_eq!(
        roles
            .iter()
            .filter(|seen| **seen == Some(StepRole::MbxCache))
            .count(),
        1
    );
    assert_owned_homes(steps, StepRole::MbxPreflight, "MBX preflight");
    assert_owned_homes(steps, StepRole::PreseedBuild, PRESEED_BUILD_NAME);
}
