use super::*;
use velnor_actions_workflow_renderer::steps::MBX_SETUP_NAME;

#[test]
fn drivers_follow_per_crate_selection() {
    let mut mbx = group("demo", TaskKind::Clippy, &[]);
    mbx.identity.compile_driver = CompileDriver::Mbx.as_str().to_owned();
    let cargo = group("nested", TaskKind::Clippy, &[]);
    let found = build_crate_jobs(
        "ubuntu-26.04",
        WorkflowPolicy::ConsumerV1,
        &discovery(vec![mbx, cargo]),
        &ToolCatalog::pinned(),
        &[],
        &[],
        None,
        2,
    )
    .expect("crate jobs");
    assert_eq!(found.drivers["rust-demo"], RenderDriver::Mbx);
    assert_eq!(found.drivers["rust-nested"], RenderDriver::Cargo);
    let steps = names(&found.jobs[0].1);
    let setup = steps
        .iter()
        .position(|name| *name == MBX_SETUP_NAME)
        .expect("MBX local setup");
    assert!(
        !steps.contains(&"Verify MBX and Rust toolchains"),
        "the renderer injects the preflight after full-job expansion: {steps:?}"
    );
    assert_eq!(
        steps.iter().filter(|name| **name == MBX_SETUP_NAME).count(),
        1,
        "MBX selection emits exactly one local setup action"
    );
    let action = &found.jobs[0].1.steps[setup];
    let velnor_actions_contract::StepKind::Action { with, .. } = &action.kind else {
        panic!("MBX setup must be an action");
    };
    assert_eq!(
        with.get("version").map(String::as_str),
        Some(velnor_actions_mise::MR_BOXINGTON_VERSION)
    );
    assert_eq!(with.get("backend").map(String::as_str), Some("local"));
    let steps = names(&found.jobs[1].1);
    assert!(!steps.contains(&MBX_SETUP_NAME), "{steps:?}");
    assert!(
        !steps.contains(&"Verify MBX and Rust toolchains"),
        "Cargo jobs have no MBX preflight: {steps:?}"
    );
}
