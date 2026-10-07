//! Checks that `logs --follow` streams and waits for the platform log reader.

#![cfg(target_os = "linux")]

use std::fs::{self, Permissions};
use std::io::{BufRead, BufReader};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

#[test]
fn logs_follow_streams_journalctl_and_waits_for_the_follower()
-> Result<(), Box<dyn std::error::Error>> {
    let scratch = TestDir::new()?;
    let bin = scratch.path().join("bin");
    fs::create_dir(&bin)?;
    let capture = scratch.path().join("journalctl-args.txt");
    let fake = bin.join("journalctl");
    fs::write(
        &fake,
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$VELNOR_TEST_CAPTURE\"\nprintf 'before-follow\\n'\n/bin/sleep 2\nprintf 'after-follow\\n'\n",
    )?;
    fs::set_permissions(&fake, Permissions::from_mode(0o755))?;

    let mut child = ChildGuard(
        Command::new(env!("CARGO_BIN_EXE_velnor-host"))
            .args(["logs", "--follow"])
            .env("PATH", &bin)
            .env("VELNOR_TEST_CAPTURE", &capture)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?,
    );
    let stdout = child.0.stdout.take().ok_or("stdout pipe missing")?;
    let (sender, receiver) = mpsc::channel();
    let reader = thread::spawn(move || -> std::io::Result<()> {
        let mut stdout = BufReader::new(stdout);
        let mut line = String::new();
        loop {
            line.clear();
            if stdout.read_line(&mut line)? == 0 {
                return Ok(());
            }
            if sender.send(line.clone()).is_err() {
                return Ok(());
            }
        }
    });
    let first_line = receiver.recv_timeout(Duration::from_secs(2))?;
    if first_line != "before-follow\n" {
        return Err(format!("unexpected first streamed line: {first_line:?}").into());
    }
    if child.0.try_wait()?.is_some() {
        return Err("logs --follow returned before the journal follower exited".into());
    }
    let second_line = receiver.recv_timeout(Duration::from_secs(3))?;
    if second_line != "after-follow\n" {
        return Err(format!("unexpected second streamed line: {second_line:?}").into());
    }
    let status = wait_bounded(&mut child.0, Duration::from_secs(3))?;
    reader
        .join()
        .map_err(|_| "stdout reader thread panicked")??;
    if !status.success() {
        return Err(format!("logs --follow exited with {status}").into());
    }
    let args: Vec<_> = fs::read_to_string(capture)?
        .lines()
        .map(str::to_owned)
        .collect();
    let expected = [
        "--unit=velnor-host.service",
        "--lines=200",
        "--no-pager",
        "--follow",
    ];
    if args != expected {
        return Err(format!("journalctl args {args:?}, expected {expected:?}").into());
    }
    Ok(())
}

fn wait_bounded(child: &mut Child, timeout: Duration) -> std::io::Result<ExitStatus> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        if Instant::now() >= deadline {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "logs --follow did not exit after the journal follower finished",
            ));
        }
        thread::sleep(Duration::from_millis(20));
    }
}

struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _killed = self.0.kill();
            let _reaped = self.0.wait();
        }
    }
}

struct TestDir(PathBuf);

impl TestDir {
    fn new() -> std::io::Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let sequence = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "velnor-cli-logs-follow-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&path)?;
        Ok(Self(path))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _removed = fs::remove_dir_all(&self.0);
    }
}
