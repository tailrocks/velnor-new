use super::*;

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
    let preflight = steps
        .iter()
        .position(|name| *name == "Verify MBX and Rust toolchains")
        .expect("MBX preflight");
    let restore = steps
        .iter()
        .position(|name| *name == "Restore MBX objects")
        .expect("MBX restore");
    assert!(
        preflight < restore,
        "MBX is checked before the action: {steps:?}"
    );
    assert_eq!(
        steps
            .iter()
            .filter(|name| **name == "Verify MBX and Rust toolchains")
            .count(),
        1,
        "every MBX action has one strict preflight"
    );
    let steps = names(&found.jobs[1].1);
    assert!(!steps.contains(&"Restore MBX objects"), "{steps:?}");
    assert!(
        !steps.contains(&"Verify MBX and Rust toolchains"),
        "Cargo jobs have no MBX preflight: {steps:?}"
    );
}
