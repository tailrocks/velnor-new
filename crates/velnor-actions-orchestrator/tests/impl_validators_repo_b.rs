//! I/O hardening cases: MBX transport layering.
//!
//! Split from `impl_validators_repo.rs` by the 400-line repo-size gate.

use std::collections::BTreeMap;

use velnor_actions_mise::cache::validate_sources_path;
use velnor_actions_workflow_renderer::steps::{
    CompileDriver, TASK_ARTIFACTS_DIR, cache_action_step, mbx_steps_for_driver,
};

use crate::impl_validators_repo::uses;

#[test]
fn velnor_sources_transport_rejects_mbx_paths() {
    assert!(validate_sources_path("registry/index/example").is_ok());
    assert!(validate_sources_path("git/db/example").is_ok());
    for bad in [
        "mbx/objects/x",
        "mbx",
        "registry/../mbx/x",
        "task-artifacts/v2/x",
        "/registry/x",
    ] {
        assert!(
            validate_sources_path(bad).is_err(),
            "sources transport must reject: {bad}"
        );
    }
}

#[test]
fn cache_action_transport_never_carries_mbx() {
    let restore = uses("actions/cache/restore");
    let save = uses("actions/cache/save");
    let key = "velnor-sources-abc";
    let sources = vec!["${{ runner.temp }}/velnor/cargo/registry/cache/x".to_owned()];
    assert!(cache_action_step(true, &restore, "sources", key, &[], &sources).is_ok());
    assert!(
        cache_action_step(
            false,
            &save,
            "task",
            key,
            &[],
            &[TASK_ARTIFACTS_DIR.to_owned()]
        )
        .is_ok()
    );
    assert!(
        cache_action_step(true, &restore, "mbx", key, &[], &sources).is_err(),
        "mbx layer must use objects mode"
    );
    // NOTE: `$CARGO_HOME/git/../mbx/x` normalizes outside the allowed
    // sources but `validate_cache_path` only checks the second segment;
    // latent `..` gap in renderer `cache_steps.rs` (not owned here),
    // reported separately. Direct MBX paths below are all rejected.
    for bad in [
        "$CARGO_HOME/mbx/cache/x",
        "$MISE_TASK_CACHE_DIR/mbx/x",
        TASK_ARTIFACTS_DIR,
    ] {
        assert!(
            cache_action_step(true, &restore, "sources", key, &[], &[bad.to_owned()]).is_err(),
            "sources layer must reject: {bad}"
        );
    }
    assert!(
        cache_action_step(
            false,
            &save,
            "task",
            key,
            &[],
            &["$MISE_TASK_CACHE_DIR/mbx/x".to_owned()]
        )
        .is_err(),
        "task layer takes only the task-artifacts dir"
    );
}

#[test]
fn mbx_transport_stays_with_mr_boxington_action() {
    let mbx = uses("jdx/mr-boxington-action");
    let pin = velnor_actions_mise::MR_BOXINGTON_VERSION;
    let rust = velnor_actions_mise::ToolCatalog::pinned()
        .version(velnor_actions_mise::PinnedTool::Rust)
        .to_owned();
    let env = BTreeMap::from([
        (
            "MISE_RUSTUP_HOME".to_owned(),
            "${{ runner.temp }}/velnor/rustup".to_owned(),
        ),
        (
            "MISE_CARGO_HOME".to_owned(),
            "${{ runner.temp }}/velnor/cargo".to_owned(),
        ),
        ("RUSTUP_TOOLCHAIN".to_owned(), rust.clone()),
    ]);
    let [preflight, step, version_check] =
        mbx_steps_for_driver(&mbx, CompileDriver::Mbx, pin, &rust, env.clone())
            .expect("objects steps")
            .expect("MBX profile");
    assert_eq!(preflight.name, "Verify Rust before MBX action");
    assert_eq!(version_check.name, "Verify native MBX version");
    assert!(
        format!("{:?}", step.kind).contains("jdx/mr-boxington-action"),
        "mbx bytes move only through the external action"
    );
    assert!(
        format!("{:?}", step.kind).contains("toolchain"),
        "action uses the catalog Rust pin after an exact preflight"
    );
    assert!(
        mbx_steps_for_driver(&mbx, CompileDriver::Cargo, pin, &rust, env.clone())
            .expect("Cargo profile")
            .is_none()
    );
    let other = uses("actions/cache/restore");
    assert!(mbx_steps_for_driver(&other, CompileDriver::Mbx, pin, &rust, env).is_err());
}
