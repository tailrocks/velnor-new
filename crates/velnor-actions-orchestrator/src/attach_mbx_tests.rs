use super::*;

#[test]
fn preseed_restores_mbx_builds_after_sources_with_homes() {
    use velnor_actions_workflow_renderer::cache_p08::SAVE_SOURCES_NAME;
    use velnor_actions_workflow_renderer::steps::MBX_SETUP_NAME;
    use velnor_actions_workflow_renderer::{PRESEED_BUILD_NAME, PRESEED_VERIFY_NAME};
    let mut plan = preseed_fixture(true, &[String::new()]);
    attach_preseed(&mut plan, "ubuntu-26.04", "0.1.0").expect("attach");
    let steps = &plan.ir.jobs["plan"].steps;
    let names: Vec<&str> = steps.iter().map(|step| step.name.as_str()).collect();
    let at = |name: &str| {
        names
            .iter()
            .position(|step| *step == name)
            .unwrap_or_else(|| panic!("missing {name}: {names:?}"))
    };
    let (restore, mbx, probe, build, verify, save) = (
        at(RESTORE_SOURCES_NAME),
        at(MBX_SETUP_NAME),
        at(crate::source_prep::FETCH_SOURCES_STEP),
        at(PRESEED_BUILD_NAME),
        at(PRESEED_VERIFY_NAME),
        at(SAVE_SOURCES_NAME),
    );
    assert!(
        restore < mbx && mbx < probe && probe < build && build < verify && verify < save,
        "preseed order: {names:?}"
    );
    assert!(
        !names.contains(&"Verify MBX and Rust toolchains"),
        "the renderer injects the preflight after full-job expansion: {names:?}"
    );
    let mbx_action = steps
        .iter()
        .find(|step| step.name == MBX_SETUP_NAME)
        .expect("MBX action");
    let velnor_actions_contract::StepKind::Action { with, .. } = &mbx_action.kind else {
        panic!("MBX setup must be an action");
    };
    assert!(
        with.contains_key("version"),
        "local setup uses the catalog-pinned MBX version"
    );
    assert_eq!(
        with.get("version").map(String::as_str),
        Some(velnor_actions_mise::MR_BOXINGTON_VERSION)
    );
    assert_eq!(with.get("backend").map(String::as_str), Some("local"));
    assert_eq!(
        names.iter().filter(|name| **name == MBX_SETUP_NAME).count(),
        1
    );
    assert_owned_homes(steps, PRESEED_BUILD_NAME);
    assert_owned_homes(steps, PRESEED_VERIFY_NAME);
}
