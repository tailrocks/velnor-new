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
    let alive = decoy.try_wait().ok().is_none_or(|status| status.is_none());
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

fn recorded_pid(dir: &Path, pid: u32) {
    std::fs::create_dir_all(dir.join("authority")).unwrap_or_else(|e| unreachable!("{e}"));
    std::fs::write(dir.join("authority/pid"), pid.to_string())
        .unwrap_or_else(|e| unreachable!("{e}"));
}

fn exited_pid() -> u32 {
    let mut child = Command::new("/usr/bin/true")
        .spawn()
        .unwrap_or_else(|e| unreachable!("{e}"));
    let _ = child.wait();
    child.id()
}

#[tokio::test]
async fn down_forgets_a_pid_with_no_binary_record_when_that_process_is_gone() {
    let dir = temp_dir("supervisor-pid-gone");
    recorded_pid(&dir, exited_pid());
    let env = Environment::at(&dir, Silent)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    let down = env.down().await;
    assert!(down.is_ok(), "{down:?}");
    assert!(!dir.join("authority/pid").exists(), "the stale pid is kept");
}

#[tokio::test]
async fn down_refuses_a_live_pid_with_no_binary_record_and_says_what_to_do() {
    let dir = temp_dir("supervisor-pid-live");
    let mut live = Command::new("/bin/sleep")
        .arg("30")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap_or_else(|e| unreachable!("{e}"));
    let pid = live.id();
    recorded_pid(&dir, pid);
    let env = Environment::at(&dir, Silent)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    let down = env.down().await;
    let alive = live.try_wait().ok().is_none_or(|status| status.is_none());
    let _ = live.kill();
    let _ = live.wait();
    let message = down.err().map(|e| e.to_string()).unwrap_or_default();
    assert!(alive, "down stopped a process without a binary record");
    assert!(
        message.contains(&format!("stop process {pid}")),
        "{message}"
    );
    assert!(message.contains("authority/pid"), "{message}");
}

#[tokio::test]
async fn down_refuses_a_pid_it_cannot_signal_with_no_binary_record() {
    let dir = temp_dir("supervisor-pid-foreign");
    recorded_pid(&dir, 1);
    let env = Environment::at(&dir, Silent)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    let down = env.down().await;
    assert!(
        down.is_err(),
        "down forgot a live pid it cannot signal: {down:?}"
    );
    assert!(
        dir.join("authority/pid").exists(),
        "the pid of a live process was forgotten"
    );
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
async fn down_keeps_every_record_when_ps_cannot_tell_whether_the_pid_is_the_authority() {
    let dir = temp_dir("supervisor-ps-fails");
    std::fs::create_dir_all(dir.join("authority")).unwrap_or_else(|e| unreachable!("{e}"));
    for (name, content) in [
        ("pid", "4294967295"),
        ("binary", "/opt/velnor/bin/velnor-local"),
        ("address", "127.0.0.1:9"),
    ] {
        std::fs::write(dir.join("authority").join(name), content)
            .unwrap_or_else(|e| unreachable!("{e}"));
    }
    let env = Environment::at(&dir, Silent)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    let down = env.down().await;
    assert!(down.is_err(), "{down:?}");
    for name in ["pid", "binary", "address"] {
        assert!(
            dir.join("authority").join(name).exists(),
            "{name} was deleted"
        );
    }
}
