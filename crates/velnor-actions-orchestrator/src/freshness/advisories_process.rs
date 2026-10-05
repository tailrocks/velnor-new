//! Bounded process supervision and output capture for the live advisory scan.

use std::io::{self, Read};
use std::process::{Child, ChildStderr, ChildStdout, Command, ExitStatus};
use std::thread;
use std::time::{Duration, Instant};

#[cfg(unix)]
use rustix::fs::{OFlags, fcntl_getfl, fcntl_setfl};

#[path = "advisories_cleanup.rs"]
mod cleanup;
pub(super) use cleanup::terminate_process_tree;

pub(super) const OUTPUT_DRAIN_LIMIT: Duration = Duration::from_millis(300);
pub(super) const CLEANUP_LIMIT: Duration = Duration::from_secs(2);
const POLL_INTERVAL: Duration = Duration::from_millis(20);
const CAPTURE_LIMIT: usize = 8 * 1024;
const READ_BUDGET: usize = 64 * 1024;
pub(super) const INTERRUPTED_READ_LIMIT: usize = 64;
type CapturedStreams = (Vec<u8>, Vec<u8>);

#[cfg(unix)]
type PreparedStreams = (Option<ChildStdout>, Option<ChildStderr>, CapturedStreams);

#[derive(Clone, Copy)]
pub(super) struct RunLimits {
    pub(super) runtime: Duration,
    pub(super) output_drain: Duration,
    pub(super) cleanup: Duration,
}

pub(super) struct LiveOutput {
    pub(super) status: Option<ExitStatus>,
    pub(super) streams: CapturedStreams,
}

pub(super) fn isolate_process_tree(command: &mut Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
}

#[cfg(unix)]
pub(super) fn run_bounded(child: &mut Child, limits: RunLimits) -> io::Result<LiveOutput> {
    let (mut stdout, mut stderr, mut streams) = prepare_streams(child, limits.cleanup)?;
    let started = Instant::now();
    supervise(
        child,
        &mut stdout,
        &mut stderr,
        &mut streams,
        started,
        limits,
    )
}

#[cfg(unix)]
fn prepare_streams(child: &mut Child, cleanup: Duration) -> io::Result<PreparedStreams> {
    let (stdout, stderr) = match take_output(child) {
        Ok(streams) => streams,
        Err(error) => {
            return Err(cleanup_failed_run(
                child,
                &mut None,
                &mut None,
                &mut (Vec::new(), Vec::new()),
                &error,
                cleanup,
                false,
            ));
        }
    };
    let mut stdout = Some(stdout);
    let mut stderr = Some(stderr);
    let mut streams = (Vec::new(), Vec::new());
    let configured = stdout
        .as_ref()
        .map_or(Ok(()), set_nonblocking)
        .and_then(|()| stderr.as_ref().map_or(Ok(()), set_nonblocking));
    if let Err(error) = configured {
        return Err(cleanup_failed_run(
            child,
            &mut stdout,
            &mut stderr,
            &mut streams,
            &error,
            cleanup,
            false,
        ));
    }
    Ok((stdout, stderr, streams))
}

#[cfg(unix)]
fn supervise(
    child: &mut Child,
    stdout: &mut Option<ChildStdout>,
    stderr: &mut Option<ChildStderr>,
    streams: &mut CapturedStreams,
    started: Instant,
    limits: RunLimits,
) -> io::Result<LiveOutput> {
    let mut status = None;
    let mut exited_at = None;
    loop {
        if let Err(error) = drain_streams(stdout, stderr, streams) {
            return Err(supervision_error(
                child,
                stdout,
                stderr,
                streams,
                &error,
                limits.cleanup,
            ));
        }
        if status.is_none() {
            match child.try_wait() {
                Ok(result) => {
                    status = result;
                    if status.is_some() {
                        exited_at = Some(Instant::now());
                    }
                }
                Err(error) => {
                    return Err(supervision_error(
                        child,
                        stdout,
                        stderr,
                        streams,
                        &error,
                        limits.cleanup,
                    ));
                }
            }
        }
        if let Some(status) = status {
            if stdout.is_none() && stderr.is_none() {
                return Ok(LiveOutput {
                    status: Some(status),
                    streams: std::mem::take(streams),
                });
            }
            if exited_at.is_some_and(|time| time.elapsed() >= limits.output_drain) {
                return finish_after_exit(
                    child,
                    status,
                    stdout.take(),
                    stderr.take(),
                    std::mem::take(streams),
                    limits.cleanup,
                );
            }
        } else if started.elapsed() >= limits.runtime {
            return finish_timeout(
                child,
                stdout.take(),
                stderr.take(),
                std::mem::take(streams),
                limits.cleanup,
            );
        }
        thread::sleep(POLL_INTERVAL);
    }
}

#[cfg(unix)]
fn supervision_error(
    child: &mut Child,
    stdout: &mut Option<ChildStdout>,
    stderr: &mut Option<ChildStderr>,
    streams: &mut CapturedStreams,
    error: &io::Error,
    cleanup: Duration,
) -> io::Error {
    cleanup_failed_run(child, stdout, stderr, streams, error, cleanup, true)
}

pub(super) fn take_output(child: &mut Child) -> io::Result<(ChildStdout, ChildStderr)> {
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("cargo-deny stdout pipe missing"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| io::Error::other("cargo-deny stderr pipe missing"))?;
    Ok((stdout, stderr))
}

#[cfg(unix)]
pub(super) fn set_nonblocking(stream: &impl std::os::fd::AsFd) -> io::Result<()> {
    let flags = fcntl_getfl(stream).map_err(io::Error::from)?;
    fcntl_setfl(stream, flags | OFlags::NONBLOCK).map_err(io::Error::from)
}

fn drain_streams(
    stdout: &mut Option<ChildStdout>,
    stderr: &mut Option<ChildStderr>,
    output: &mut CapturedStreams,
) -> io::Result<()> {
    drain_stream(stdout, &mut output.0)?;
    drain_stream(stderr, &mut output.1)
}

pub(super) fn drain_stream(stream: &mut Option<impl Read>, output: &mut Vec<u8>) -> io::Result<()> {
    let mut chunk = [0_u8; 8192];
    let mut drained = 0;
    let mut interrupted = 0;
    while stream.is_some() && drained < READ_BUDGET {
        let result = stream
            .as_mut()
            .map_or(Ok(0), |reader| reader.read(&mut chunk));
        match result {
            Ok(0) => *stream = None,
            Ok(count) => {
                append_tail(output, &chunk[..count]);
                drained += count;
                interrupted = 0;
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {
                interrupted += 1;
                if interrupted >= INTERRUPTED_READ_LIMIT {
                    break;
                }
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

pub(super) fn append_tail(output: &mut Vec<u8>, bytes: &[u8]) {
    if bytes.len() >= CAPTURE_LIMIT {
        output.clear();
        output.extend_from_slice(&bytes[bytes.len() - CAPTURE_LIMIT..]);
        return;
    }
    let overflow = output
        .len()
        .saturating_add(bytes.len())
        .saturating_sub(CAPTURE_LIMIT);
    if overflow > 0 {
        output.drain(..overflow);
    }
    output.extend_from_slice(bytes);
}

fn finish_timeout(
    child: &mut Child,
    mut stdout: Option<ChildStdout>,
    mut stderr: Option<ChildStderr>,
    mut streams: CapturedStreams,
    cleanup: Duration,
) -> io::Result<LiveOutput> {
    let deadline = Instant::now() + cleanup;
    if let Err(error) = terminate_process_tree(child, deadline) {
        return Err(cleanup_failed_run(
            child,
            &mut stdout,
            &mut stderr,
            &mut streams,
            &error,
            cleanup,
            true,
        ));
    }
    wait_until(child, deadline)?
        .ok_or_else(|| io::Error::other("timed out during process cleanup"))?;
    drain_until(&mut stdout, &mut stderr, &mut streams, deadline)?;
    Ok(LiveOutput {
        status: None,
        streams,
    })
}

fn finish_after_exit(
    child: &mut Child,
    status: ExitStatus,
    mut stdout: Option<ChildStdout>,
    mut stderr: Option<ChildStderr>,
    mut streams: CapturedStreams,
    cleanup: Duration,
) -> io::Result<LiveOutput> {
    let deadline = Instant::now() + cleanup;
    if let Err(error) = terminate_process_tree(child, deadline) {
        return Err(cleanup_failed_run(
            child,
            &mut stdout,
            &mut stderr,
            &mut streams,
            &error,
            cleanup,
            true,
        ));
    }
    if !drain_until(&mut stdout, &mut stderr, &mut streams, deadline)? {
        return Err(io::Error::other(
            "output pipes remained open after process-tree cleanup",
        ));
    }
    Ok(LiveOutput {
        status: Some(status),
        streams,
    })
}

fn drain_until(
    stdout: &mut Option<ChildStdout>,
    stderr: &mut Option<ChildStderr>,
    streams: &mut CapturedStreams,
    deadline: Instant,
) -> io::Result<bool> {
    while (stdout.is_some() || stderr.is_some()) && Instant::now() < deadline {
        drain_streams(stdout, stderr, streams)?;
        if stdout.is_some() || stderr.is_some() {
            thread::sleep(POLL_INTERVAL);
        }
    }
    Ok(stdout.is_none() && stderr.is_none())
}

pub(super) fn wait_until(child: &mut Child, deadline: Instant) -> io::Result<Option<ExitStatus>> {
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(Some(status));
        }
        if Instant::now() >= deadline {
            return Ok(None);
        }
        thread::sleep(POLL_INTERVAL.min(deadline.saturating_duration_since(Instant::now())));
    }
}

pub(super) fn cleanup_failed_run(
    child: &mut Child,
    stdout: &mut Option<ChildStdout>,
    stderr: &mut Option<ChildStderr>,
    streams: &mut CapturedStreams,
    source: &io::Error,
    cleanup: Duration,
    drain: bool,
) -> io::Error {
    let deadline = Instant::now() + cleanup;
    let mut detail = source.to_string();
    if let Err(error) = terminate_process_tree(child, deadline) {
        append_error(&mut detail, "; cleanup failed: ", &error);
    }
    match wait_until(child, deadline) {
        Ok(Some(_)) => {}
        Ok(None) => detail.push_str("; child cleanup timed out"),
        Err(error) => append_error(&mut detail, "; child wait failed: ", &error),
    }
    if drain && let Err(error) = drain_until(stdout, stderr, streams, deadline) {
        append_error(&mut detail, "; output drain failed: ", &error);
    }
    io::Error::other(detail)
}

pub(super) fn append_error(detail: &mut String, prefix: &str, error: &io::Error) {
    detail.push_str(prefix);
    detail.push_str(&error.to_string());
}
