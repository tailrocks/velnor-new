//! Layout admission chooses one declared executable shape and retains bytes by move.
use super::*;
use std::time::Duration;

fn deadline() -> velnor_actions_mise::CheckDeadline {
    velnor_actions_mise::CheckDeadline::after(Duration::from_secs(60)).expect("deadline")
}

fn tool() -> QualifiedTool {
    serde_json::from_value(serde_json::json!({
        "id":"node","backend":{"kind":"core","tool":"node"},"version":"24.1.0",
        "options":{"kind":"default"},"depends_on":[],"platforms":[{
            "platform":"linux_x64","artifacts":[{"url":"https://nodejs.org/dist/v24.1.0/node-v24.1.0-linux-x64.tar.xz","sha256":"a".repeat(64)}],
            "dependency_artifacts":[],"install_tree_sha256":"b".repeat(64),
            "executables":[{"name":"node","path":"bin/node","sha256":"c".repeat(64),
                "probe":{"kind":"version","expected":"node 24.1.0"}}]}]
    })).expect("tool")
}
fn executable(root: &Path) {
    std::fs::create_dir_all(root.join("bin")).expect("bin");
    std::fs::write(root.join("bin/node"), b"fixture bytes").expect("executable");
}

#[test]
fn unique_wrapper_moves_into_prefix_without_duplicate_payload() {
    let temp = tempfile::TempDir::new().expect("temp");
    let root = temp.path().join("unpacked");
    let wrapped = root.join("node-release");
    executable(&wrapped);
    let prefix = temp.path().join("prefix");
    normalize_payload(
        &tool(),
        CheckPlatform::LinuxX64,
        &[root],
        &prefix,
        deadline(),
    )
    .expect("normalize");
    assert!(!wrapped.exists());
    assert_eq!(
        std::fs::read(prefix.join("bin/node")).expect("bytes"),
        b"fixture bytes"
    );
}

#[test]
fn ambiguous_layout_and_existing_destination_fail_before_move() {
    let temp = tempfile::TempDir::new().expect("temp");
    let root = temp.path().join("unpacked");
    executable(&root);
    executable(&root.join("wrapper"));
    let prefix = temp.path().join("prefix");
    assert!(
        normalize_payload(
            &tool(),
            CheckPlatform::LinuxX64,
            std::slice::from_ref(&root),
            &prefix,
            deadline(),
        )
        .is_err()
    );
    assert!(root.join("bin/node").exists());
    std::fs::remove_dir_all(root.join("wrapper")).expect("remove ambiguity");
    std::fs::create_dir(&prefix).expect("existing");
    assert!(
        normalize_payload(
            &tool(),
            CheckPlatform::LinuxX64,
            &[root],
            &prefix,
            deadline()
        )
        .is_err()
    );
}
