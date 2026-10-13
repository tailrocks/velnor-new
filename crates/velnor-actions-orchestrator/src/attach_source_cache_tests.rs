use super::*;
use velnor_actions_workflow_renderer::steps::MBX_RESTORE_NAME;

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
    let source_restore = names
        .iter()
        .position(|step| *step == RESTORE_SOURCES_NAME)
        .expect("shared Cargo source restore");
    let probe = names
        .iter()
        .position(|step| *step == crate::source_prep::FETCH_SOURCES_STEP)
        .expect("sources step");
    let native_preflight = names
        .iter()
        .position(|step| *step == MBX_PREFLIGHT_NAME)
        .expect("native Rust preflight for the helper build");
    let native_owner = names
        .iter()
        .position(|step| *step == MBX_RESTORE_NAME)
        .expect("native MBX owner for the helper build");
    let version_check = names
        .iter()
        .position(|step| *step == MBX_VERSION_CHECK_NAME)
        .expect("native MBX version guard");
    let build = names
        .iter()
        .position(|step| *step == PRESEED_BUILD_NAME)
        .expect("build step");
    let source_save = names
        .iter()
        .position(|step| *step == SAVE_SOURCES_NAME)
        .expect("shared Cargo source save");
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
        names
            .iter()
            .filter(|step| **step == MBX_RESTORE_NAME)
            .count(),
        1,
        "the native action is the only MBX owner"
    );
    assert!(
        steps.iter().all(|step| !matches!(
            &step.kind,
            velnor_actions_contract::StepKind::Shell { run, .. }
                if run.iter().any(|argument| argument == "mr-boxington@1.24.0")
        )),
        "the pre-seed lane must not install action-owned MBX through Mise"
    );
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
