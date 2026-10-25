use super::{BoundedOutput, BoundedProcessError, CLEANUP_RESERVE, POLL_INTERVAL, READ_CHUNK};
use group::{GroupAnchor, ProcessState, observe_process};
use rustix::fs::{OFlags, fcntl_getfl, fcntl_setfl};
use rustix::io::{Errno, read};
use rustix::process::Pid;
use std::io;
use std::os::fd::AsFd;
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::process::{Child, ChildStderr, ChildStdout, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

#[cfg(test)]
mod descendant_tests;
mod group;

enum StopCause {
    TimedOut,
    OutputLimit,
    Io(io::Error),
}

impl StopCause {
    fn into_error(self) -> BoundedProcessError {
        match self {
            Self::TimedOut => BoundedProcessError::TimedOut,
            Self::OutputLimit => BoundedProcessError::OutputLimit,
            Self::Io(error) => BoundedProcessError::Io(error),
        }
    }
}

struct Capture {
    stdout_pipe: Option<ChildStdout>,
    stderr_pipe: Option<ChildStderr>,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    cap: usize,
    exceeded: bool,
}

impl Capture {
    fn new(child: &mut Child, cap: usize) -> Self {
        Self {
            stdout_pipe: child.stdout.take(),
            stderr_pipe: child.stderr.take(),
            stdout: Vec::new(),
            stderr: Vec::new(),
            cap,
            exceeded: false,
        }
    }

    fn make_nonblocking(&self) -> io::Result<()> {
        set_nonblocking(self.stdout_pipe.as_ref())?;
        set_nonblocking(self.stderr_pipe.as_ref())
    }

    fn drain(&mut self) -> io::Result<()> {
        drain_pipe(
            &mut self.stdout_pipe,
            &mut self.stdout,
            self.cap,
            &mut self.exceeded,
        )?;
        drain_pipe(
            &mut self.stderr_pipe,
            &mut self.stderr,
            self.cap,
            &mut self.exceeded,
        )
    }

    fn close_pipes(&mut self) {
        self.stdout_pipe.take();
        self.stderr_pipe.take();
    }

    fn closed(&self) -> bool {
        self.stdout_pipe.is_none() && self.stderr_pipe.is_none()
    }

    fn into_output(self, status: ExitStatus) -> BoundedOutput {
        BoundedOutput {
            stdout: self.stdout,
            stderr: self.stderr,
            success: status.success(),
            code: status.code(),
            signal: status.signal(),
        }
    }
}

pub(super) fn run_bounded(
    command: &mut Command,
    cap: usize,
    timeout: Duration,
) -> Result<BoundedOutput, BoundedProcessError> {
    if timeout.is_zero() {
        return Err(BoundedProcessError::TimedOut);
    }
    let started = Instant::now();
    let Some(deadline) = started.checked_add(timeout) else {
        return Err(BoundedProcessError::TimedOut);
    };
    let reserve = CLEANUP_RESERVE.min(timeout / 4);
    let run_deadline = deadline.checked_sub(reserve).unwrap_or(started);

    let mut group = GroupAnchor::spawn(deadline)?;
    if Instant::now() >= run_deadline {
        return Err(stop_anchor_with_cause(
            &mut group,
            deadline,
            StopCause::TimedOut,
        ));
    }
    if let Err(error) = group.ensure_alive(run_deadline) {
        return Err(stop_anchor_with_failure(&mut group, deadline, error));
    }

    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(group.pid().as_raw_pid());
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            return Err(stop_anchor_with_cause(
                &mut group,
                deadline,
                StopCause::Io(error),
            ));
        }
    };
    let pid = Pid::from_child(&child);
    let mut capture = Capture::new(&mut child, cap);
    if let Err(error) = group.ensure_alive(run_deadline) {
        capture.close_pipes();
        return Err(stop_target_with_failure(
            &mut group, &mut child, pid, None, deadline, error,
        ));
    }
    if let Err(error) = capture.make_nonblocking() {
        capture.close_pipes();
        return Err(stop_target_with_cause(
            &mut group,
            &mut child,
            pid,
            None,
            deadline,
            StopCause::Io(error),
        ));
    }

    wait_for_target(&mut group, &mut child, pid, capture, run_deadline, deadline)
}

fn wait_for_target(
    group: &mut GroupAnchor,
    child: &mut Child,
    pid: Pid,
    mut capture: Capture,
    run_deadline: Instant,
    deadline: Instant,
) -> Result<BoundedOutput, BoundedProcessError> {
    loop {
        if let Err(error) = capture.drain() {
            return Err(stop_target_with_cause(
                group,
                child,
                pid,
                Some(&mut capture),
                deadline,
                StopCause::Io(error),
            ));
        }
        if capture.exceeded {
            return Err(stop_target_with_cause(
                group,
                child,
                pid,
                Some(&mut capture),
                deadline,
                StopCause::OutputLimit,
            ));
        }
        if Instant::now() >= run_deadline {
            return Err(stop_target_with_cause(
                group,
                child,
                pid,
                Some(&mut capture),
                deadline,
                StopCause::TimedOut,
            ));
        }
        if let Err(error) = group.ensure_alive(run_deadline) {
            return Err(stop_target_with_failure(
                group,
                child,
                pid,
                Some(&mut capture),
                deadline,
                error,
            ));
        }
        match observe_process(pid, run_deadline) {
            Ok(ProcessState::Exited) => {
                return finish_success(group, child, pid, capture, deadline);
            }
            Ok(ProcessState::Running) => {}
            Err(error) => {
                return Err(stop_target_with_cause(
                    group,
                    child,
                    pid,
                    Some(&mut capture),
                    deadline,
                    StopCause::Io(error),
                ));
            }
        }
        pause_until(run_deadline);
    }
}

fn finish_success(
    group: &mut GroupAnchor,
    child: &mut Child,
    pid: Pid,
    mut capture: Capture,
    deadline: Instant,
) -> Result<BoundedOutput, BoundedProcessError> {
    let status = group
        .stop_with_target(child, pid, Some(&mut capture), deadline)
        .map_err(BoundedProcessError::Cleanup)?;
    if Instant::now() >= deadline {
        return Err(BoundedProcessError::Cleanup(timeout_error()));
    }
    if capture.exceeded {
        return Err(BoundedProcessError::OutputLimit);
    }
    Ok(capture.into_output(status))
}

fn stop_anchor_with_cause(
    group: &mut GroupAnchor,
    deadline: Instant,
    cause: StopCause,
) -> BoundedProcessError {
    match group.stop(deadline) {
        Ok(()) => cause.into_error(),
        Err(error) => BoundedProcessError::Cleanup(error),
    }
}

fn stop_anchor_with_failure(
    group: &mut GroupAnchor,
    deadline: Instant,
    failure: io::Error,
) -> BoundedProcessError {
    match group.stop(deadline) {
        Ok(()) => BoundedProcessError::Cleanup(failure),
        Err(error) => BoundedProcessError::Cleanup(error),
    }
}

fn stop_target_with_cause(
    group: &mut GroupAnchor,
    child: &mut Child,
    pid: Pid,
    capture: Option<&mut Capture>,
    deadline: Instant,
    cause: StopCause,
) -> BoundedProcessError {
    match group.stop_with_target(child, pid, capture, deadline) {
        Ok(_) => cause.into_error(),
        Err(error) => BoundedProcessError::Cleanup(error),
    }
}

fn stop_target_with_failure(
    group: &mut GroupAnchor,
    child: &mut Child,
    pid: Pid,
    capture: Option<&mut Capture>,
    deadline: Instant,
    failure: io::Error,
) -> BoundedProcessError {
    match group.stop_with_target(child, pid, capture, deadline) {
        Ok(_) => BoundedProcessError::Cleanup(failure),
        Err(error) => BoundedProcessError::Cleanup(error),
    }
}

fn set_nonblocking<Fd: AsFd>(pipe: Option<&Fd>) -> io::Result<()> {
    if let Some(pipe) = pipe {
        let flags = fcntl_getfl(pipe).map_err(io::Error::from)?;
        fcntl_setfl(pipe, flags | OFlags::NONBLOCK).map_err(io::Error::from)?;
    }
    Ok(())
}

fn drain_pipe<Fd: AsFd>(
    pipe: &mut Option<Fd>,
    output: &mut Vec<u8>,
    cap: usize,
    exceeded: &mut bool,
) -> io::Result<()> {
    let Some(fd) = pipe.as_ref() else {
        return Ok(());
    };
    let room = cap.saturating_sub(output.len());
    let length = if *exceeded {
        READ_CHUNK
    } else {
        room.saturating_add(1).min(READ_CHUNK)
    };
    let mut buffer = [0; READ_CHUNK];
    match read(fd, &mut buffer[..length]) {
        Ok(0) => {
            pipe.take();
        }
        Ok(_) if *exceeded => {}
        Ok(length) if length > room => {
            *exceeded = true;
        }
        Ok(length) => output.extend_from_slice(&buffer[..length]),
        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
        Err(error) if error == Errno::INTR => {}
        Err(error) => {
            pipe.take();
            return Err(io::Error::from(error));
        }
    }
    Ok(())
}

pub(super) fn timeout_error() -> io::Error {
    io::Error::new(io::ErrorKind::TimedOut, "process cleanup deadline expired")
}

fn pause_until(deadline: Instant) {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if !remaining.is_zero() {
        thread::sleep(remaining.min(POLL_INTERVAL));
    }
}
