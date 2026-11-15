// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::future::Future;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use velnor_local::io::env::{Engine, Environment};
use velnor_local::io::supervisor;

use crate::support::temp_dir;

struct Silent;

impl Engine for Silent {
    fn docker(&self, _: &[&str]) -> impl Future<Output = io::Result<String>> + Send {
        std::future::ready(Ok(String::new()))
    }

    fn logged(&self, _: &[&str], _: &Path) -> impl Future<Output = io::Result<bool>> + Send {
        std::future::ready(Ok(true))
    }
}

fn mode(path: &Path) -> u32 {
    std::fs::metadata(path).map_or_else(
        |e| unreachable!("{e}"),
        |meta| meta.permissions().mode() & 0o777,
    )
}

#[tokio::test]
async fn down_does_not_stop_a_process_whose_command_only_looks_like_the_authority() {
    let dir = temp_dir("supervisor-lookalike");
    let canonical = std::fs::canonicalize(&dir).unwrap_or_else(|e| unreachable!("{e}"));
    let mut decoy: Child = Command::new("/bin/sh")
        .args(["-c", "read line", "supervise", "--dir"])
        .arg(&canonical)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap_or_else(|e| unreachable!("{e}"));
    std::fs::create_dir_all(dir.join("authority")).unwrap_or_else(|e| unreachable!("{e}"));
    std::fs::write(dir.join("authority/pid"), decoy.id().to_string())
        .unwrap_or_else(|e| unreachable!("{e}"));
    std::fs::write(dir.join("authority/binary"), "/usr/local/bin/velnor-local")
        .unwrap_or_else(|e| unreachable!("{e}"));
    let env = Environment::at(&dir, Silent)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    let down = env.down().await;
    let alive = decoy.try_wait().map_or(true, |status| status.is_none());
    let _ = decoy.kill();
    let _ = decoy.wait();
    assert!(down.is_ok(), "{down:?}");
    assert!(alive, "down stopped a process that is not the authority");
}

#[tokio::test]
async fn down_stops_the_authority_recorded_at_start_by_its_exact_command() {
    let dir = temp_dir("supervisor-standin");
    let canonical = std::fs::canonicalize(&dir).unwrap_or_else(|e| unreachable!("{e}"));
    let binary = "/opt/velnor/bin/velnor-local";
    let child = Command::new("/usr/bin/yes")
        .arg0(binary)
        .args(["supervise", "--dir"])
        .arg(&canonical)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap_or_else(|e| unreachable!("{e}"));
    let pid = child.id();
    let (reaped, exited) = mpsc::channel();
    std::thread::spawn(move || {
        let mut child = child;
        let _ = reaped.send(child.wait());
    });
    std::fs::create_dir_all(dir.join("authority")).unwrap_or_else(|e| unreachable!("{e}"));
    std::fs::write(dir.join("authority/pid"), pid.to_string())
        .unwrap_or_else(|e| unreachable!("{e}"));
    std::fs::write(dir.join("authority/binary"), binary).unwrap_or_else(|e| unreachable!("{e}"));
    let env = Environment::at(&dir, Silent)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    let down = env.down().await;
    let stopped = exited.recv_timeout(Duration::from_secs(5)).is_ok();
    if !stopped {
        let _ = Command::new("kill")
            .args(["-KILL", &pid.to_string()])
            .status();
    }
    assert!(down.is_ok(), "{down:?}");
    assert!(stopped, "down did not stop the recorded authority");
}

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
