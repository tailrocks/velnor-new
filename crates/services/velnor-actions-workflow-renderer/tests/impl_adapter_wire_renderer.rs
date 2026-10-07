//! Adapter-wire cases: toolchain-home env, bare-cargo scans, lane target
//! dirs, cache order, and release gates (F2 halves).
use std::collections::BTreeMap;

use velnor_actions_contract_config::WorkflowPolicy;
use velnor_actions_workflow_cache::cache_steps::check_cache_step_order;
use velnor_actions_workflow_cache::cache_steps::{
    TOOLS_CACHE_PATH, TOOLS_RESTORE_USES, TOOLS_SAVE_USES, cache_action_step,
};
use velnor_actions_workflow_jobs::check_release_build;
use velnor_actions_workflow_renderer::lane_target::lane_cargo_target_env;
use velnor_actions_workflow_renderer::render_workflow_ir;
use velnor_actions_workflow_steps::toolchain_env::{
    TOOLCHAIN_HOME_KEYS, check_toolchain_homes, with_toolchain_homes,
};
use velnor_actions_workflow_steps::{RenderError, check_no_bare_cargo};

use super::impl_renderer_fixtures::{fixture_ctx, fixture_ir, job, mise_argv, scrubbed_shell_step};

#[test]
fn toolchain_homes_merge_and_validate() {
    assert_eq!(
        TOOLCHAIN_HOME_KEYS,
        ["MISE_RUSTUP_HOME", "MISE_CARGO_HOME", "RUSTUP_TOOLCHAIN"]
    );
    let base = BTreeMap::from([("MATRIX_KEY".to_owned(), "m".to_owned())]);
    let merged = with_toolchain_homes(&base, "/r/rustup", "/r/cargo", "1.98.1");
    assert_eq!(merged.get("MATRIX_KEY").map(String::as_str), Some("m"));
    assert_eq!(check_toolchain_homes(&merged), Ok(()));
    assert!(check_toolchain_homes(&base).is_err());
    let mut blank = merged.clone();
    blank.insert("RUSTUP_TOOLCHAIN".to_owned(), String::new());
    assert!(check_toolchain_homes(&blank).is_err());
}

#[test]
fn bare_cargo_scan_rejects_unpinned_rust() {
    let pinned = "      run: mise --no-config exec rust@1.98.1 -- cargo --version\n";
    assert_eq!(check_no_bare_cargo(pinned), Ok(()));
    let bare = "      run: cargo --version\n";
    let err = check_no_bare_cargo(bare).expect_err("bare cargo");
    assert!(err.to_string().contains("bare_rust_invocation:cargo"));
    let block =
        "      run: |\n        mise exec rust@1.98.1 -- cargo test\n        mbx build --locked\n";
    let err = check_no_bare_cargo(block).expect_err("bare mbx");
    assert!(err.to_string().contains("bare_rust_invocation:mbx"));
    let names = "    - name: Run cargo-deny\n      uses: example/example@aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n";
    assert_eq!(check_no_bare_cargo(names), Ok(()));
    let path = "        path: $CARGO_HOME/registry\n";
    assert_eq!(check_no_bare_cargo(path), Ok(()));
    assert_eq!(check_no_bare_cargo("      run: echo hello\n"), Ok(()));
}

#[test]
fn bare_cargo_scan_covers_rendered_workflow() -> Result<(), RenderError> {
    let ctx = fixture_ctx();
    let argv = mise_argv("rust@1.98.1", "cargo", &["--version"]);
    let step = scrubbed_shell_step("Run cargo", argv)?;
    let ir = fixture_ir(vec![job("task", "Task", Vec::new(), vec![step])]);
    let rendered = render_workflow_ir(&ir, WorkflowPolicy::ConsumerV1, None, &ctx)?;
    assert_eq!(check_no_bare_cargo(&rendered), Ok(()));
    let bare = scrubbed_shell_step(
        "Run cargo",
        vec!["cargo".to_owned(), "--version".to_owned()],
    )?;
    let ir = fixture_ir(vec![job("task", "Task", Vec::new(), vec![bare])]);
    let rendered = render_workflow_ir(&ir, WorkflowPolicy::ConsumerV1, None, &ctx)?;
    assert!(check_no_bare_cargo(&rendered).is_err());
    Ok(())
}

#[test]
fn lane_env_isolates_target_dir() {
    assert_eq!(
        lane_cargo_target_env("3"),
        (
            "CARGO_TARGET_DIR".to_owned(),
            "$RUNNER_TEMP/velnor/target/3".to_owned()
        )
    );
}

#[test]
fn cache_steps_restore_before_save() -> Result<(), RenderError> {
    let restore = cache_action_step(
        true,
        TOOLS_RESTORE_USES,
        "tools",
        "k",
        &["rk".to_owned()],
        &[TOOLS_CACHE_PATH.to_owned()],
    )?;
    let save = cache_action_step(
        false,
        TOOLS_SAVE_USES,
        "tools",
        "k",
        &[],
        &[TOOLS_CACHE_PATH.to_owned()],
    )?;
    assert_eq!(
        check_cache_step_order(&[restore.clone(), save.clone()]),
        Ok(())
    );
    assert_eq!(check_cache_step_order(std::slice::from_ref(&save)), Ok(()));
    assert!(check_cache_step_order(&[save, restore]).is_err());
    Ok(())
}

#[test]
fn release_build_rejects_non_release() {
    assert_eq!(check_release_build("0.1.0", "policy"), Ok(()));
    let err = check_release_build("0.1.0-rc.1", "policy").expect_err("prerelease");
    assert!(err.to_string().contains("non_release_build"));
    assert!(check_release_build("", "policy").is_err());
}
