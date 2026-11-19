// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use velnor_local::io::supervisor;

use super::mode;
use crate::support::temp_dir;

#[tokio::test]
async fn start_creates_the_keys_folder_private() {
    let dir = temp_dir("supervisor-keys-fresh");
    let _ = supervisor::start(&dir).await;
    assert_eq!(mode(&dir.join("keys")), 0o700);
}

#[tokio::test]
async fn start_tightens_a_keys_folder_that_is_looser() {
    let dir = temp_dir("supervisor-keys-loose");
    std::fs::create_dir_all(dir.join("keys")).unwrap_or_else(|e| unreachable!("{e}"));
    std::fs::set_permissions(dir.join("keys"), std::fs::Permissions::from_mode(0o755))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let _ = supervisor::start(&dir).await;
    assert_eq!(mode(&dir.join("keys")), 0o700);
}

#[tokio::test]
async fn start_creates_the_authority_folder_private() {
    let dir = temp_dir("supervisor-authority-fresh");
    let _ = supervisor::start(&dir).await;
    assert_eq!(mode(&dir.join("authority")), 0o700);
}

#[tokio::test]
async fn start_tightens_an_authority_folder_that_is_looser() {
    let dir = temp_dir("supervisor-authority-loose");
    std::fs::create_dir_all(dir.join("authority")).unwrap_or_else(|e| unreachable!("{e}"));
    std::fs::set_permissions(
        dir.join("authority"),
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap_or_else(|e| unreachable!("{e}"));
    let _ = supervisor::start(&dir).await;
    assert_eq!(mode(&dir.join("authority")), 0o700);
}

#[tokio::test]
async fn a_cancelled_start_stops_the_authority_process_it_spawned() {
    let dir = temp_dir("supervisor-cancel");
    let script = dir.join("fake-authority");
    std::fs::write(&script, "#!/bin/sh\nexec /bin/sleep 600\n")
        .unwrap_or_else(|e| unreachable!("{e}"));
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let cancelled = tokio::time::timeout(
        Duration::from_millis(600),
        supervisor::start_binary(&dir, &script),
    )
    .await;
    assert!(cancelled.is_err(), "the fake authority should never answer");
    let pid = std::fs::read_to_string(dir.join("authority/pid")).unwrap_or_default();
    let pid = pid.trim().to_owned();
    assert!(
        pid.parse::<u32>().is_ok(),
        "the cancelled start recorded no pid: {pid:?}"
    );
    let mut gone = false;
    for _ in 0..40 {
        let state = Command::new("ps")
            .args(["-p", &pid, "-o", "stat="])
            .output()
            .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_owned())
            .unwrap_or_default();
        if state.is_empty() || state.starts_with('Z') {
            gone = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    if !gone {
        let _ = Command::new("kill").args(["-KILL", &pid]).status();
    }
    assert!(gone, "process {pid} outlived the cancelled start");
}

#[tokio::test]
async fn a_plain_start_clears_the_plan_of_an_earlier_pair_run() {
    let dir = temp_dir("supervisor-stale-plan");
    std::fs::create_dir_all(dir.join("runner")).unwrap_or_else(|e| unreachable!("{e}"));
    std::fs::write(dir.join("runner/plan.json"), "{}").unwrap_or_else(|e| unreachable!("{e}"));
    let started = supervisor::start_binary(&dir, Path::new("/usr/bin/false")).await;
    assert!(started.is_err());
    assert!(!dir.join("runner/plan.json").exists());
}
