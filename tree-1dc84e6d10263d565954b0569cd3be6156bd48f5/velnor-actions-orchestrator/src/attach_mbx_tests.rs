use super::*;

#[test]
fn preseed_restores_mbx_builds_after_sources_with_homes() {
    use velnor_actions_contract::StepRole;
    use velnor_actions_workflow_renderer::{
        MBX_PREFLIGHT_NAME, PRESEED_BUILD_NAME, PRESEED_VERIFY_NAME,
    };
    let mut plan = preseed_fixture(true, &[String::new()]);
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
    let (restore, preflight, mbx, probe, build, verify, save) = (
        at(StepRole::CargoSourcesRestore),
        at(StepRole::MbxPreflight),
        at(StepRole::MbxCache),
        at(StepRole::CargoSourcesFetch),
        at(StepRole::PreseedBuild),
        at(StepRole::PreseedVerifyBuild),
        at(StepRole::CargoSourcesSave),
    );
    assert!(
        restore < preflight
            && preflight < mbx
            && mbx < probe
            && probe < build
            && build < verify
            && verify < save,
        "preseed order: {names:?}"
    );
    assert_owned_homes(steps, MBX_PREFLIGHT_NAME);
    let mbx_action = steps
        .iter()
        .find(|step| step.role == Some(StepRole::MbxCache))
        .expect("MBX action");
    let velnor_actions_contract::StepKind::Action { env, with, .. } = &mbx_action.kind else {
        panic!("MBX restore must be an action");
    };
    assert_eq!(with.get("toolchain").map(String::as_str), Some("1.98.1"));
    assert!(
        !with.contains_key("version"),
        "preflight owns MBX install identity"
    );
    assert_eq!(env.get("RUSTUP_HOME"), env.get("MISE_RUSTUP_HOME"));
    assert_eq!(env.get("CARGO_HOME"), env.get("MISE_CARGO_HOME"));
    assert_owned_homes(steps, PRESEED_BUILD_NAME);
    assert_owned_homes(steps, PRESEED_VERIFY_NAME);
}
