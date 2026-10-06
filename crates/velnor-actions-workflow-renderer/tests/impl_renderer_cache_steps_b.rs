//! Gate 4 renderer cases, second half: lane isolation and MBX object gating.

use velnor_actions_contract::StepKind;
use velnor_actions_workflow_renderer::steps::{
    CompileDriver, MBX_CACHE_MODE_ENV, mbx_steps_for_driver, target_dir_for_lane,
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
fn mbx_objects_step_gates_save_to_push_via_cache_mode() {
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
        let StepKind::Action { env, .. } = &step.kind else {
            panic!("mbx must be an action step");
        };
        assert_eq!(
            env.get(MBX_CACHE_MODE_ENV).map(String::as_str),
            Some(
                "${{ github.event_name == 'push' && github.ref == format('refs/heads/{0}', github.event.repository.default_branch) && github.ref_protected == true && 'write' || 'read' }}"
            ),
            "native object saves are limited to protected default-branch pushes"
        );
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
