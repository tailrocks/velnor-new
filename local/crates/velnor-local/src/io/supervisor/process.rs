// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io;

use nix::errno::Errno;
use nix::sys::signal::{Signal, kill};
use nix::unistd::Pid;
#[cfg(not(target_os = "linux"))]
use std::process::Stdio;
#[cfg(target_os = "linux")]
use tokio::fs;
#[cfg(not(target_os = "linux"))]
use tokio::process::Command;

fn pid_number(pid: &str) -> io::Result<i32> {
    match pid.parse::<i32>() {
        Ok(number) if number > 0 => Ok(number),
        _ => Err(io::Error::other(format!("invalid pid {pid}"))),
    }
}

pub(super) fn send(pid: &str, signal: Signal) -> io::Result<()> {
    let number = pid_number(pid)?;
    match kill(Pid::from_raw(number), signal) {
        Ok(()) | Err(Errno::ESRCH) => Ok(()),
        Err(error) => Err(io::Error::other(format!("kill {signal} {pid}: {error}"))),
    }
}

// Linux reads the kernel's record directly, so `down` needs no `ps` in a
// minimal image. Other platforms have no /proc and keep using `ps`.
#[cfg(target_os = "linux")]
pub(super) async fn command_of(pid: &str) -> io::Result<Option<String>> {
    let number = pid_number(pid)?;
    match fs::read(format!("/proc/{number}/cmdline")).await {
        Ok(bytes) => {
            let bytes = bytes.strip_suffix(&[0]).unwrap_or(&bytes);
            let args: Vec<_> = bytes
                .split(|byte| *byte == 0)
                .map(String::from_utf8_lossy)
                .collect();
            Ok(Some(args.join(" ")))
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

#[cfg(not(target_os = "linux"))]
pub(super) async fn command_of(pid: &str) -> io::Result<Option<String>> {
    let output = Command::new("ps")
        .args(["-p", pid, "-o", "command="])
        .stdin(Stdio::null())
        .output()
        .await?;
    if output.status.success() {
        let line = String::from_utf8_lossy(&output.stdout);
        return Ok(Some(line.trim().to_owned()));
    }
    if output.status.code() == Some(1) && output.stderr.is_empty() {
        return Ok(None);
    }
    let error = String::from_utf8_lossy(&output.stderr);
    Err(io::Error::other(format!(
        "ps -p {pid} -o command=: {}",
        error.trim()
    )))
}
