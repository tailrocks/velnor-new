use super::*;
use velnor_actions_workflow_renderer::steps::{MBX_PREFLIGHT_NAME, MBX_RESTORE_NAME};

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
        None,
        2,
    )
    .expect("crate jobs");
    assert_eq!(found.drivers["rust-demo"], RenderDriver::Mbx);
    assert_eq!(found.drivers["rust-nested"], RenderDriver::Cargo);
    let steps = names(&found.jobs[0].1);
    let preflight = steps
        .iter()
        .position(|name| *name == MBX_PREFLIGHT_NAME)
        .expect("MBX preflight");
    let setup = steps
        .iter()
        .position(|name| *name == MBX_RESTORE_NAME)
        .expect("MBX objects restore");
    assert!(
        preflight < setup,
        "preflight precedes MBX action: {steps:?}"
    );
    assert_eq!(
        steps
            .iter()
            .filter(|name| **name == MBX_RESTORE_NAME)
            .count(),
        1,
        "MBX selection emits exactly one objects action"
    );
    let action = &found.jobs[0].1.steps[setup];
    let velnor_actions_contract::StepKind::Action { with, env, .. } = &action.kind else {
        panic!("MBX setup must be an action");
    };
    assert_eq!(
        env.get("VELNOR_MBX_VERSION").map(String::as_str),
        Some(velnor_actions_mise::MR_BOXINGTON_VERSION)
    );
    assert!(!with.contains_key("version"));
    let steps = names(&found.jobs[1].1);
    assert!(!steps.contains(&MBX_PREFLIGHT_NAME), "{steps:?}");
    assert!(!steps.contains(&MBX_RESTORE_NAME), "{steps:?}");
}
