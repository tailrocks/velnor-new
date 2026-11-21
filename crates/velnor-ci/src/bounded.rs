// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::{self, ErrorKind, Read};
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use nix::sys::signal::{Signal, killpg};
use nix::unistd::Pid;

const POLL: Duration = Duration::from_millis(10);

#[derive(Debug)]
pub struct Captured {
    pub success: bool,
    pub stdout: Vec<u8>,
}

pub fn capture(command: &mut Command, limit: u64, timeout: Duration) -> io::Result<Captured> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let pipe = child
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("no stdout"))?;
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        pipe.take(limit.saturating_add(1))
            .read_to_end(&mut bytes)
            .map(|_| bytes)
    });
    let deadline = Instant::now() + timeout;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            child.kill().ok();
            child.wait().ok();
            return Err(io::ErrorKind::TimedOut.into());
        }
        std::thread::sleep(POLL);
    };
    let stdout = reader
        .join()
        .map_err(|_| io::Error::other("reader panicked"))??;
    Ok(Captured {
        success: status.success(),
        stdout,
    })
}

const DRAIN: Duration = Duration::from_secs(5);

#[derive(Debug)]
pub struct Streams {
    pub status: ExitStatus,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

#[derive(Debug)]
pub enum Outcome {
    Exited(Streams),
    TimedOut,
    Oversized,
}

pub fn capture_streams(
    command: &mut Command,
    stdout_limit: usize,
    stderr_keep: usize,
    deadline: &dyn Fn() -> Option<Instant>,
) -> io::Result<Outcome> {
    let mut child = command
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let over = Arc::new(AtomicBool::new(false));
    let stdout = child
        .stdout
        .take()
        .map(|pipe| drain(pipe, stdout_limit, Some(Arc::clone(&over))));
    let stderr = child
        .stderr
        .take()
        .map(|pipe| drain(pipe, stderr_keep, None));
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if over.load(Ordering::Acquire) {
            return stop(&mut child, Outcome::Oversized);
        }
        if deadline().is_some_and(|at| Instant::now() >= at) {
            return stop(&mut child, Outcome::TimedOut);
        }
        std::thread::sleep(POLL);
    };
    let stdout = stdout.and_then(|receiver| receiver.recv_timeout(DRAIN).ok());
    let stderr = stderr.and_then(|receiver| receiver.recv_timeout(DRAIN).ok());
    match (stdout, stderr) {
        (Some(_), Some(_)) if over.load(Ordering::Acquire) => Ok(Outcome::Oversized),
        (Some(stdout), Some(stderr)) => Ok(Outcome::Exited(Streams {
            status,
            stdout,
            stderr,
        })),
        _ => Ok(Outcome::TimedOut),
    }
}

fn stop(child: &mut Child, outcome: Outcome) -> io::Result<Outcome> {
    if let Ok(group) = i32::try_from(child.id()) {
        killpg(Pid::from_raw(group), Signal::SIGKILL).ok();
    }
    child.kill().ok();
    child.wait()?;
    Ok(outcome)
}

fn drain(
    mut pipe: impl Read + Send + 'static,
    keep: usize,
    stop_when_full: Option<Arc<AtomicBool>>,
) -> mpsc::Receiver<Vec<u8>> {
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let mut kept = Vec::new();
        let mut chunk = [0_u8; 8192];
        loop {
            let read = match pipe.read(&mut chunk) {
                Ok(read @ 1..) => read,
                Err(error) if error.kind() == ErrorKind::Interrupted => continue,
                Ok(0) | Err(_) => break,
            };
            let room = keep.saturating_sub(kept.len());
            kept.extend_from_slice(&chunk[..read.min(room)]);
            if read > room
                && let Some(over) = &stop_when_full
            {
                over.store(true, Ordering::Release);
                break;
            }
        }
        sender.send(kept).ok();
    });
    receiver
}

#[cfg(test)]
mod tests {
    use super::drain;
    use std::io::{self, Read};

    struct InterruptedOnce {
        interrupted: bool,
        data: &'static [u8],
    }

    impl Read for InterruptedOnce {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            if !self.interrupted {
                self.interrupted = true;
                return Err(io::ErrorKind::Interrupted.into());
            }
            let count = self.data.len().min(buffer.len());
            buffer[..count].copy_from_slice(&self.data[..count]);
            self.data = &self.data[count..];
            Ok(count)
        }
    }

    #[test]
    fn an_interrupted_read_is_retried_and_the_data_after_it_is_captured() {
        let receiver = drain(
            InterruptedOnce {
                interrupted: false,
                data: b"listing",
            },
            100,
            None,
        );
        assert_eq!(receiver.recv().ok(), Some(b"listing".to_vec()));
    }
}
