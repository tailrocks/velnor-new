//! Tool snapshot capture/verify and finding-line rendering.

use velnor_actions_orchestrator_discovery::tool_snapshot::ToolSnapshot;
use velnor_actions_orchestrator_discovery::toolfindings::tool_check_lines;

#[test]
fn capture_then_verify_is_clean() {
    let dir = tempfile::tempdir().expect("tempdir");
    let snap = ToolSnapshot::capture(dir.path());
    snap.verify(dir.path()).expect("clean verifies");
}

#[test]
fn capture_is_stable_for_unchanged_tree() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("mise.toml"), "[tools]\n").expect("tool file");
    assert_eq!(
        ToolSnapshot::capture(dir.path()),
        ToolSnapshot::capture(dir.path())
    );
}

#[test]
fn edited_tool_file_fails_verify() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("mise.toml"), "[tools]\n").expect("tool file");
    let snap = ToolSnapshot::capture(dir.path());
    std::fs::write(dir.path().join("mise.toml"), "[tools]\nchanged\n").expect("edit");
    let err = snap.verify(dir.path()).expect_err("drift must fail");
    assert!(err.to_string().contains("mise.toml"), "{err}");
}

#[test]
fn added_tool_file_fails_verify() {
    let dir = tempfile::tempdir().expect("tempdir");
    let snap = ToolSnapshot::capture(dir.path());
    std::fs::write(dir.path().join("rust-toolchain.toml"), "[toolchain]\n").expect("add");
    snap.verify(dir.path()).expect_err("new file must fail");
}

#[test]
fn no_checks_render_no_lines() {
    assert!(tool_check_lines(&[]).is_empty());
}
