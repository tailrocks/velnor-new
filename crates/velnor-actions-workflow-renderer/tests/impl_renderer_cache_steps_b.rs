//! Gate 4 renderer cases, second half: lane isolation and MBX object gating.

use velnor_actions_contract::StepKind;
use velnor_actions_workflow_renderer::steps::{
    CompileDriver, mbx_steps_for_driver, target_dir_for_lane,
};

use super::impl_renderer_fixtures::*;

/// Pinned refs used across cache-step cases.
fn sha() -> &'static str {
    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
}

#[test]
fn lane_target_dirs_stay_isolated() {
    let one = target_dir_for_lane("lane-one");
    let two = target_dir_for_lane("lane-two");
    assert!(one.starts_with("$RUNNER_TEMP/velnor/target/"), "{one}");
    assert_ne!(one, two, "lanes never share a target dir");
}

#[test]
fn mbx_action_carries_no_cache_write_gate() {
    let uses = format!("jdx/mr-boxington-action@{}", sha());
    let [_, direct, _] = mbx_tool_steps(&uses, "1.19.0", "1.98.1").expect("direct MBX steps");
    let [_, driven, _] = mbx_steps_for_driver(
        &uses,
        CompileDriver::Mbx,
        "1.19.0",
        "1.98.1",
        mbx_tool_env("1.98.1"),
    )
    .expect("driver MBX")
    .expect("MBX driver emits");
    for step in [&direct, &driven] {
        let StepKind::Action { env, with, .. } = &step.kind else {
            panic!("mbx must be an action step");
        };
        assert!(!env.contains_key("ACTIONS_CACHE_MODE"));
        assert_eq!(with.get("backend").map(String::as_str), Some("local"));
    }
    assert!(
        mbx_steps_for_driver(
            &uses,
            CompileDriver::Cargo,
            "1.19.0",
            "1.98.1",
            mbx_tool_env("1.98.1"),
        )
        .expect("cargo driver")
        .is_none(),
        "cargo drivers emit no preflight or MBX step to gate"
    );
}
