// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#![cfg(target_os = "linux")]

use std::future::Future;
use std::io;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use velnor_local::io::env::{Engine, Environment};

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

// Minimal images ship no `ps` or `kill`. Re-run this test with an empty PATH,
// so any program that `down` tries to start fails to spawn.
const WITHOUT_PATH: &str = "VELNOR_TEST_DOWN_WITHOUT_PATH";

#[tokio::test]
async fn down_needs_no_ps_or_kill_on_path() {
    if std::env::var_os(WITHOUT_PATH).is_none() {
        let empty = temp_dir("supervisor-empty-path");
        let me = std::env::current_exe().unwrap_or_else(|e| unreachable!("{e}"));
        let status = Command::new(me)
            .args([
                "--exact",
                "supervisor_down::down_needs_no_ps_or_kill_on_path",
            ])
            .env_clear()
            .env("PATH", &empty)
            .env(WITHOUT_PATH, "1")
            .status()
            .unwrap_or_else(|e| unreachable!("{e}"));
        assert!(status.success(), "down needs a program on PATH: {status}");
        return;
    }
    let dir = temp_dir("supervisor-no-ps");
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
        let _ = Command::new("/bin/kill")
            .args(["-KILL", &pid.to_string()])
            .status();
    }
    assert!(down.is_ok(), "{down:?}");
    assert!(stopped, "down did not stop the recorded authority");
}
