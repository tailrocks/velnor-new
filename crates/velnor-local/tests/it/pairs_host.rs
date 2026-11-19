// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_local::images::HOST_BINARY;

use crate::support::temp_dir;

fn installed(root: &std::path::Path, mode: u32) -> (std::path::PathBuf, std::path::PathBuf) {
    use std::os::unix::fs::PermissionsExt;
    let exe = root.join("target/release/velnor-local");
    let host = root.join(HOST_BINARY);
    for path in [&exe, &host] {
        let parent = path.parent().unwrap_or_else(|| unreachable!());
        std::fs::create_dir_all(parent).unwrap_or_else(|e| unreachable!("{e}"));
        std::fs::write(path, "#!/bin/sh\n").unwrap_or_else(|e| unreachable!("{e}"));
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
            .unwrap_or_else(|e| unreachable!("{e}"));
    }
    (exe, host)
}

#[tokio::test]
async fn the_default_velnor_host_is_the_one_built_beside_the_installed_executable() {
    let root = temp_dir("pairs-host-installed");
    let (exe, host) = installed(&root, 0o755);
    assert_eq!(
        velnor_local::io::supervisor::default_host(&exe).await.ok(),
        Some(host)
    );
}

#[tokio::test]
async fn a_velnor_host_planted_in_the_working_directory_is_never_the_default() {
    let planted = temp_dir("pairs-host-planted");
    installed(&planted, 0o755);
    let elsewhere = temp_dir("pairs-host-elsewhere");
    let exe = elsewhere.join("bin/velnor-local");
    std::fs::create_dir_all(exe.parent().unwrap_or_else(|| unreachable!()))
        .unwrap_or_else(|e| unreachable!("{e}"));
    std::fs::write(&exe, "").unwrap_or_else(|e| unreachable!("{e}"));
    let resolved = velnor_local::io::supervisor::default_host(&exe).await;
    assert!(resolved.is_err(), "{resolved:?}");
}

#[tokio::test]
async fn a_default_velnor_host_that_others_can_write_is_refused() {
    let root = temp_dir("pairs-host-writable");
    let (exe, _) = installed(&root, 0o775);
    let resolved = velnor_local::io::supervisor::default_host(&exe).await;
    assert!(resolved.is_err(), "{resolved:?}");
}

#[test]
fn the_cli_never_picks_a_velnor_host_planted_in_its_working_directory() {
    let cwd = temp_dir("pairs-cli-cwd");
    let planted = cwd.join(HOST_BINARY);
    let installed = temp_dir("pairs-cli-installed").join("bin/velnor-local");
    for dir in [planted.parent(), installed.parent()] {
        std::fs::create_dir_all(dir.unwrap_or_else(|| unreachable!()))
            .unwrap_or_else(|e| unreachable!("{e}"));
    }
    std::fs::write(&planted, "#!/bin/sh\n").unwrap_or_else(|e| unreachable!("{e}"));
    std::fs::set_permissions(
        &planted,
        std::os::unix::fs::PermissionsExt::from_mode(0o755),
    )
    .unwrap_or_else(|e| unreachable!("{e}"));
    std::fs::copy(env!("CARGO_BIN_EXE_velnor-local"), &installed)
        .unwrap_or_else(|e| unreachable!("{e}"));
    let output = std::process::Command::new(&installed)
        .current_dir(&cwd)
        .args(["up", "--repo", ".", "--dir"])
        .arg(cwd.join("state"))
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{stderr}");
    assert!(
        stderr.contains(&format!("no {HOST_BINARY} in any folder above")),
        "{stderr}"
    );
}

#[tokio::test]
async fn a_default_host_candidate_that_cannot_be_inspected_is_an_error_not_a_miss() {
    use std::os::unix::fs::PermissionsExt;
    let root = temp_dir("pairs-host-unreadable");
    let (exe, _) = installed(&root, 0o755);
    let blocked = root.join("target");
    std::fs::set_permissions(&blocked, std::fs::Permissions::from_mode(0o000))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let resolved = velnor_local::io::supervisor::default_host(&exe).await;
    std::fs::set_permissions(&blocked, std::fs::Permissions::from_mode(0o755))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let error = resolved
        .err()
        .unwrap_or_else(|| unreachable!("resolved a host"));
    assert_eq!(
        error.kind(),
        std::io::ErrorKind::PermissionDenied,
        "{error}"
    );
}
