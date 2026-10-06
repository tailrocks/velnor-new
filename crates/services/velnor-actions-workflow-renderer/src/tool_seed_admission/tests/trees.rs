use super::super::*;
use super::{scratch, seed, trust_script};
use crate::tool_seed_test_support::mock_trust_commands;
use std::fs;
use std::process::Command;

#[test]
fn tree_scan_rejects_depth_count_and_find_errors_without_truncation() {
    let test_dir = scratch("bounds");
    let guard = trusted_seed_guard(SEED_ROOT).expect("guard");
    let script = format!("{guard}\ntrusted_seed_tree_bounded \"$SEED_TEST_ROOT\"");
    let script = mock_trust_commands(&script, &test_dir);
    let run_mode = |mode: &str| {
        Command::new("bash")
            .arg("-c")
            .arg(format!("set -euo pipefail\n{script}"))
            .env("RUNNER_OS", "Linux")
            .env("SEED_TEST_ROOT", test_dir.join("virtual-seed"))
            .env("SEED_TEST_FIND_MODE", mode)
            .output()
            .expect("run bounds check")
    };
    assert!(run_mode("tree-deep-ok").status.success());
    assert!(!run_mode("tree-deep-bad").status.success());
    assert!(!run_mode("tree-too-many").status.success());
    assert!(!run_mode("tree-error").status.success());
    fs::remove_dir_all(test_dir).ok();
}

#[test]
fn symlinks_and_non_root_entries_fail_tree_admission() {
    let test_dir = scratch("entries");
    let seed_root = test_dir.join("seed");
    seed(&seed_root);
    let child = seed_root.join("mise/tree/untrusted");
    fs::write(&child, "data").expect("child");
    let uid = Command::new("id").arg("-u").output().expect("current uid");
    if String::from_utf8_lossy(&uid.stdout).trim() == "0" {
        let status = Command::new("chown")
            .arg("65534")
            .arg(&child)
            .status()
            .expect("chown");
        assert!(status.success(), "could not prepare non-root fixture");
    }
    let guard = trusted_seed_guard(SEED_ROOT).expect("guard");
    let command = format!("{guard}\ntrusted_seed_tree_entries_are_safe \"$SEED_TEST_ROOT\"");
    let output = Command::new("bash")
        .arg("-c")
        .arg(&command)
        .env("RUNNER_OS", "Linux")
        .env("SEED_TEST_ROOT", &seed_root)
        .output()
        .expect("run ownership scan");
    assert!(!output.status.success(), "accepted non-root tree entries");
    fs::remove_file(child).expect("remove foreign file");
    std::os::unix::fs::symlink("/etc/passwd", seed_root.join("mise/tree/link")).expect("symlink");
    let script = mock_trust_commands(
        &format!("{guard}\ntrusted_seed_is_trusted \"$SEED_TEST_ROOT\""),
        &test_dir,
    );
    let mounts = format!("{} ext4 0:77 ro", seed_root.display());
    let output = Command::new("bash")
        .arg("-c")
        .arg(format!("set -euo pipefail\n{script}"))
        .env("RUNNER_OS", "Linux")
        .env("SEED_TEST_ROOT", &seed_root)
        .env("SEED_TEST_MOUNTS", &mounts)
        .env("SEED_TEST_SKIP_OWNER_SCAN", "1")
        .output()
        .expect("run symlink admission");
    assert!(!output.status.success(), "accepted symlink");
    fs::remove_file(seed_root.join("mise/tree/link")).expect("remove link");
    let fifo = seed_root.join("mise/tree/fifo");
    let status = Command::new("mkfifo").arg(&fifo).status().expect("mkfifo");
    assert!(status.success(), "could not prepare FIFO fixture");
    let script = mock_trust_commands(
        &format!("{guard}\ntrusted_seed_is_trusted \"$SEED_TEST_ROOT\""),
        &test_dir,
    );
    let output = Command::new("bash")
        .arg("-c")
        .arg(format!("set -euo pipefail\n{script}"))
        .env("RUNNER_OS", "Linux")
        .env("SEED_TEST_ROOT", &seed_root)
        .env("SEED_TEST_MOUNTS", &mounts)
        .env("SEED_TEST_SKIP_OWNER_SCAN", "1")
        .output()
        .expect("run special-file admission");
    assert!(!output.status.success(), "accepted FIFO");
    fs::remove_dir_all(test_dir).ok();
}

#[test]
fn key_file_match_rejects_nul_extra_lines_and_oversize_inputs() {
    let test_dir = scratch("key-file");
    let key_file = test_dir.join("KEY");
    let guard = trusted_seed_guard(SEED_ROOT).expect("guard");
    let script = mock_trust_commands(
        &format!("{guard}\ntrusted_seed_file_matches \"$SEED_TEST_FILE\" expected"),
        &test_dir,
    );
    fs::write(&key_file, "expected").expect("plain key");
    let run_match = |size: &str| {
        Command::new("bash")
            .arg("-c")
            .arg(format!("set -euo pipefail\n{script}"))
            .env("RUNNER_OS", "Linux")
            .env("SEED_TEST_FILE", &key_file)
            .env("SEED_TEST_FILE_SIZE", size)
            .status()
            .expect("run key comparison")
    };
    assert!(run_match("64").success());
    fs::write(&key_file, b"expected\0").expect("NUL key");
    assert!(!run_match("64").success());
    fs::write(&key_file, "expected\nextra").expect("multi-line key");
    assert!(!run_match("64").success());
    fs::write(&key_file, "expected").expect("bounded key");
    assert!(!run_match("513").success());
    fs::remove_dir_all(test_dir).expect("cleanup");
}

#[test]
fn false_owner_and_mount_commands_fail_closed() {
    let test_dir = scratch("commands");
    let seed_root = test_dir.join("seed");
    seed(&seed_root);
    let script = trust_script(&test_dir);
    let exact = format!("{} ext4 0:77 ro", seed_root.display());
    let uid = Command::new("bash")
        .arg("-c")
        .arg(format!("set -euo pipefail\n{script}"))
        .env("RUNNER_OS", "Linux")
        .env("SEED_TEST_ROOT", &seed_root)
        .env("SEED_TEST_MOUNTS", &exact)
        .env("SEED_TEST_UID", "65534")
        .output()
        .expect("owner check");
    assert!(!uid.status.success(), "accepted non-root seed root");
    let mount_error = Command::new("bash")
        .arg("-c")
        .arg(format!("set -euo pipefail\n{script}"))
        .env("RUNNER_OS", "Linux")
        .env("SEED_TEST_ROOT", &seed_root)
        .env("SEED_TEST_MOUNTS", &exact)
        .env("SEED_TEST_FINDMNT_FAIL", "1")
        .output()
        .expect("mount check");
    assert!(
        !mount_error.status.success(),
        "accepted mount check failure"
    );
    fs::remove_dir_all(test_dir).ok();
}
