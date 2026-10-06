use super::*;

#[test]
fn cargo_only_preseed_replaces_registry_cache_and_uses_native_helper_owner() {
    use velnor_actions_contract_workflow::StepRole;
    use velnor_actions_workflow_renderer::{PRESEED_BUILD_NAME, PRESEED_VERIFY_NAME};
    let roots = [String::new()];
    let mut plan = preseed_fixture(false, &roots);
    attach_preseed(&mut plan, "ubuntu-26.04", "0.1.0", &roots).expect("attach");
    let steps = &plan.ir.jobs["plan"].steps;
    let names: Vec<&str> = steps.iter().map(|step| step.name.as_str()).collect();
    assert!(
        !steps
            .iter()
            .any(|step| step.role == Some(StepRole::CargoRegistryRestore)),
        "the overlapping registry cache is replaced: {names:?}"
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
        "the preseed helper installs MBX once"
    );
    assert_owned_homes(steps, StepRole::PreseedBuild, PRESEED_BUILD_NAME);
    assert_owned_homes(steps, StepRole::PreseedVerifyBuild, PRESEED_VERIFY_NAME);
}
