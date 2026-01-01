use super::*;

#[test]
fn cargo_only_preseed_uses_exact_sources_and_native_helper_owners() {
    use velnor_actions_contract::StepRole;
    use velnor_actions_workflow_renderer::{PRESEED_BUILD_NAME, PRESEED_VERIFY_NAME};
    let roots = [String::new()];
    let mut plan = preseed_fixture(&roots);
    attach_preseed(&mut plan, "ubuntu-26.04", "0.1.0").expect("attach");
    let steps = &plan.ir.jobs["plan"].steps;
    let names: Vec<&str> = steps.iter().map(|step| step.name.as_str()).collect();
    assert!(
        steps.iter().all(|step| !matches!(
            &step.kind,
            velnor_actions_contract::StepKind::Action { uses, .. }
                if uses.starts_with("Swatinem/rust-cache@")
        )),
        "retired action has no producer: {names:?}"
    );
    let role_at = |role| {
        steps
            .iter()
            .position(|step| step.role == Some(role))
            .unwrap_or_else(|| panic!("missing {role:?}: {names:?}"))
    };
    let source_restore = role_at(StepRole::CargoSourcesRestore);
    let probe = role_at(StepRole::CargoSourcesFetch);
    let native_preflight = role_at(StepRole::MbxPreflight);
    let native_owner = role_at(StepRole::MbxCache);
    let version_check = role_at(StepRole::MbxVersionCheck);
    let build = role_at(StepRole::PreseedBuild);
    let source_save = role_at(StepRole::CargoSourcesSave);
    assert!(
        source_restore < native_preflight
            && native_preflight < native_owner
            && native_owner < version_check
            && version_check < probe
            && probe < build
            && build < source_save,
        "Cargo-only tasks keep the shared source owner and the helper has one native MBX owner: {names:?}"
    );
    assert_eq!(
        steps
            .iter()
            .filter(|step| step.role == Some(StepRole::MbxCache))
            .count(),
        1,
        "the native action is the only MBX owner"
    );
    assert!(
        steps.iter().all(|step| !matches!(
            &step.kind,
            velnor_actions_contract::StepKind::Shell { run, .. }
                if run.iter().any(|argument| argument == "mr-boxington@1.21.1")
        )),
        "the pre-seed lane must not install action-owned MBX through Mise"
    );
    assert_owned_homes(steps, StepRole::PreseedBuild, PRESEED_BUILD_NAME);
    assert_owned_homes(steps, StepRole::PreseedVerifyBuild, PRESEED_VERIFY_NAME);
}
