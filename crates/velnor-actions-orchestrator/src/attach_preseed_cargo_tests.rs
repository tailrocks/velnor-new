//! Bounded regression cases.
use super::*;

#[test]
fn preseed_skips_mbx_restore_for_cargo_only_plans() {
    use velnor_actions_workflow_renderer::{PRESEED_BUILD_NAME, PRESEED_VERIFY_NAME};
    let mut plan = preseed_fixture(false, &[String::new()]);
    attach_preseed(&mut plan, "ubuntu-26.04", "0.1.0").expect("attach");
    let steps = &plan.ir.jobs["plan"].steps;
    let names: Vec<&str> = steps.iter().map(|step| step.name.as_str()).collect();
    assert!(
        !names.contains(&MBX_RESTORE_NAME),
        "cargo-only plans stay rust-cache-only: {names:?}"
    );
    let probe = names
        .iter()
        .position(|step| *step == crate::source_prep::FETCH_SOURCES_STEP)
        .expect("sources step");
    let build = names
        .iter()
        .position(|step| *step == PRESEED_BUILD_NAME)
        .expect("build step");
    assert!(probe < build, "build anchors after sources: {names:?}");
    assert_owned_homes(steps, PRESEED_BUILD_NAME);
    assert_owned_homes(steps, PRESEED_VERIFY_NAME);
}
