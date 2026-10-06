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
    let roles: Vec<Option<StepRole>> = steps.iter().map(|step| step.role).collect();
    let at = |role| {
        roles
            .iter()
            .position(|seen| *seen == Some(role))
            .unwrap_or_else(|| panic!("missing {role:?}: {roles:?}"))
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
                if run.iter().any(|argument| argument == "mr-boxington@1.21.1")
        )),
        "Mise must not install action-owned MBX: {names:?}"
    );
    assert_owned_homes(steps, StepRole::MbxPreflight, "MBX preflight");
    let mbx_action = steps
        .iter()
        .find(|step| step.role == Some(StepRole::MbxCache))
        .expect("MBX action");
    let velnor_actions_contract::StepKind::Action { env, with, .. } = &mbx_action.kind else {
        panic!("MBX restore must be an action");
    };
    assert_eq!(with.get("toolchain").map(String::as_str), Some("1.98.1"));
    assert!(
        with.get("version").map(String::as_str)
            == Some(ToolCatalog::pinned().version(PinnedTool::MrBoxington)),
        "the native action installs the catalog-pinned MBX version"
    );
    assert_eq!(env.get("RUSTUP_HOME"), env.get("MISE_RUSTUP_HOME"));
    assert_eq!(env.get("CARGO_HOME"), env.get("MISE_CARGO_HOME"));
    assert_owned_homes(steps, StepRole::PreseedBuild, PRESEED_BUILD_NAME);
    assert_owned_homes(steps, StepRole::PreseedVerifyBuild, PRESEED_VERIFY_NAME);
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
