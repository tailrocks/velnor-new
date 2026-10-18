use super::{PIPE_POLL_INTERVAL, StageError};
use std::fs::File;
use std::io::{ErrorKind, Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::{Duration, Instant};

enum FilterProgress {
    Input(Result<(), StageError>),
    Output(Result<(), StageError>),
}

/// The deadline limits the child and pipes. Regular-file I/O can delay return.
pub(super) fn run_filter(
    command: &mut Command,
    input: File,
    output: &mut File,
    input_limit: u64,
    output_limit: u64,
    timeout: Duration,
) -> Result<(), StageError> {
    let input_metadata = input
        .metadata()
        .map_err(|_| "cache_tar_archive_read_failed")?;
    let output_metadata = output
        .metadata()
        .map_err(|_| "cache_tar_archive_write_failed")?;
    if !input_metadata.is_file() || !output_metadata.is_file() {
        return Err("cache_tar_archive_invalid");
    }
    run_filter_io(command, input, output, input_limit, output_limit, timeout)
}

pub(super) fn run_filter_io<R, W>(
    command: &mut Command,
    input: R,
    output: &mut W,
    input_limit: u64,
    output_limit: u64,
    timeout: Duration,
) -> Result<(), StageError>
where
    R: Read + Send,
    W: Write + Send,
{
    use std::os::unix::process::CommandExt;

    let deadline = Instant::now()
        .checked_add(timeout)
        .ok_or("cache_tar_compression_timeout")?;
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .map_err(|_| "cache_tar_compression_failed")?;
    let group = child.id();
    let Some(child_input) = child.stdin.take() else {
        terminate_and_reap(&mut child, group);
        return Err("cache_tar_compression_failed");
    };
    let Some(child_output) = child.stdout.take() else {
        drop(child_input);
        terminate_and_reap(&mut child, group);
        return Err("cache_tar_compression_failed");
    };
    if set_nonblocking(&child_input).is_err() || set_nonblocking(&child_output).is_err() {
        drop(child_input);
        drop(child_output);
        terminate_and_reap(&mut child, group);
        return Err("cache_tar_compression_failed");
    }

    let cancelled = Arc::new(AtomicBool::new(false));
    transfer_and_wait(FilterJob {
        child: &mut child,
        group,
        input,
        output,
        child_input,
        child_output,
        input_limit,
        output_limit,
        deadline,
        cancelled,
    })
}

struct FilterJob<'a, R, W> {
    child: &'a mut Child,
    group: u32,
    input: R,
    output: &'a mut W,
    child_input: std::process::ChildStdin,
    child_output: std::process::ChildStdout,
    input_limit: u64,
    output_limit: u64,
    deadline: Instant,
    cancelled: Arc<AtomicBool>,
}

fn transfer_and_wait<R, W>(job: FilterJob<'_, R, W>) -> Result<(), StageError>
where
    R: Read + Send,
    W: Write + Send,
{
    let FilterJob {
        child,
        group,
        input,
        output,
        mut child_input,
        mut child_output,
        input_limit,
        output_limit,
        deadline,
        cancelled,
    } = job;
    let (sender, receiver) = mpsc::channel();

    thread::scope(|scope| {
        let input_cancelled = Arc::clone(&cancelled);
        let input_sender = sender.clone();
        scope.spawn(move || {
            let result = copy_into_child(input, &mut child_input, input_limit, &input_cancelled);
            drop(input_sender.send(FilterProgress::Input(result)));
        });
        let output_cancelled = Arc::clone(&cancelled);
        scope.spawn(move || {
            let result =
                copy_from_child(&mut child_output, output, output_limit, &output_cancelled);
            drop(sender.send(FilterProgress::Output(result)));
        });

        let mut input_done = false;
        let mut output_done = false;
        let mut exit_status = None;
        let mut failure = None;
        while !(input_done && output_done && exit_status.is_some()) {
            match receiver.try_recv() {
                Ok(FilterProgress::Input(Ok(()))) => input_done = true,
                Ok(FilterProgress::Output(Ok(()))) => output_done = true,
                Ok(FilterProgress::Input(Err(problem)) | FilterProgress::Output(Err(problem))) => {
                    failure = Some(problem);
                    break;
                }
                Err(mpsc::TryRecvError::Disconnected) if input_done && output_done => {}
                Err(mpsc::TryRecvError::Disconnected) => {
                    failure = Some("cache_tar_compression_failed");
                    break;
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
            if exit_status.is_none() {
                match observe_exit_without_reaping(child) {
                    Ok(Some(success)) => {
                        if !success {
                            failure = Some("cache_tar_compression_failed");
                            break;
                        }
                        exit_status = Some(());
                    }
                    Ok(None) => {}
                    Err(()) => {
                        failure = Some("cache_tar_compression_failed");
                        break;
                    }
                }
            }
            if Instant::now() >= deadline {
                failure = Some("cache_tar_compression_timeout");
                break;
            }
            thread::sleep(PIPE_POLL_INTERVAL);
        }

        if let Some(problem) = failure {
            cancelled.store(true, Ordering::Release);
            terminate_and_reap(child, group);
            Err(problem)
        } else {
            match child.wait() {
                Ok(status) if status.success() => Ok(()),
                _ => Err("cache_tar_compression_failed"),
            }
        }
    })
}

fn observe_exit_without_reaping(child: &Child) -> Result<Option<bool>, ()> {
    use rustix::process::{Pid, WaitId, WaitIdOptions, waitid};

    let raw_pid = i32::try_from(child.id()).map_err(|_| ())?;
    let pid = Pid::from_raw(raw_pid).ok_or(())?;
    let status = waitid(
        WaitId::Pid(pid),
        WaitIdOptions::NOHANG | WaitIdOptions::EXITED | WaitIdOptions::NOWAIT,
    )
    .map_err(|_| ())?;
    Ok(status.map(|status| status.exit_status() == Some(0)))
}

fn copy_into_child<R: Read, W: Write>(
    mut input: R,
    child_input: &mut W,
    limit: u64,
    cancelled: &AtomicBool,
) -> Result<(), StageError> {
    let mut total = 0_u64;
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        if cancelled.load(Ordering::Acquire) {
            return Err("cache_tar_compression_failed");
        }
        let count = input
            .read(&mut buffer)
            .map_err(|_| "cache_tar_archive_read_failed")?;
        if count == 0 {
            return Ok(());
        }
        total = total
            .checked_add(u64::try_from(count).map_err(|_| "cache_tar_archive_limit")?)
            .filter(|total| *total <= limit)
            .ok_or("cache_tar_archive_limit")?;
        write_nonblocking(child_input, &buffer[..count], cancelled)?;
    }
}

fn copy_from_child<W: Write>(
    child_output: &mut impl Read,
    output: &mut W,
    limit: u64,
    cancelled: &AtomicBool,
) -> Result<(), StageError> {
    let mut total = 0_u64;
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        if cancelled.load(Ordering::Acquire) {
            return Err("cache_tar_compression_failed");
        }
        match child_output.read(&mut buffer) {
            Ok(0) => return Ok(()),
            Ok(count) => {
                total = total
                    .checked_add(u64::try_from(count).map_err(|_| "cache_tar_archive_limit")?)
                    .filter(|total| *total <= limit)
                    .ok_or("cache_tar_archive_limit")?;
                output
                    .write_all(&buffer[..count])
                    .map_err(|_| "cache_tar_archive_write_failed")?;
            }
            Err(error) if error.kind() == ErrorKind::WouldBlock => {
                thread::sleep(PIPE_POLL_INTERVAL);
            }
            Err(error) if error.kind() == ErrorKind::Interrupted => {}
            Err(_) => return Err("cache_tar_compression_failed"),
        }
    }
}

fn write_nonblocking<W: Write>(
    writer: &mut W,
    mut bytes: &[u8],
    cancelled: &AtomicBool,
) -> Result<(), StageError> {
    while !bytes.is_empty() {
        if cancelled.load(Ordering::Acquire) {
            return Err("cache_tar_compression_failed");
        }
        match writer.write(bytes) {
            Ok(0) => return Err("cache_tar_compression_failed"),
            Ok(count) => bytes = &bytes[count..],
            Err(error) if error.kind() == ErrorKind::WouldBlock => {
                thread::sleep(PIPE_POLL_INTERVAL);
            }
            Err(error) if error.kind() == ErrorKind::Interrupted => {}
            Err(_) => return Err("cache_tar_compression_failed"),
        }
    }
    Ok(())
}

fn set_nonblocking(fd: &impl std::os::fd::AsFd) -> Result<(), ()> {
    use rustix::fs::{OFlags, fcntl_getfl, fcntl_setfl};

    let flags = fcntl_getfl(fd).map_err(|_| ())?;
    fcntl_setfl(fd, flags | OFlags::NONBLOCK).map_err(|_| ())
}

fn terminate_and_reap(child: &mut Child, group: u32) {
    use rustix::io::Errno;
    use rustix::process::{Pid, Signal, kill_process_group};

    #[cfg(test)]
    if group_signal_test_hook::observe_and_suppress(group) {
        drop(child.wait());
        return;
    }

    if let Ok(raw) = i32::try_from(group)
        && let Some(pid) = Pid::from_raw(raw)
        && let Err(error) = kill_process_group(pid, Signal::KILL)
        && error != Errno::SRCH
    {
        drop(child.kill());
    }
    drop(child.wait());
}

#[cfg(test)]
pub(in crate::cache_action_stage::compression) mod group_signal_test_hook {
    use std::cell::Cell;

    thread_local! {
        static ENABLED: Cell<bool> = const { Cell::new(false) };
        static LEADER_ALREADY_REAPED: Cell<bool> = const { Cell::new(false) };
    }

    pub(in crate::cache_action_stage::compression) fn enable() {
        ENABLED.with(|enabled| enabled.set(true));
        LEADER_ALREADY_REAPED.with(|observed| observed.set(false));
    }

    pub(in crate::cache_action_stage::compression) fn disable() {
        ENABLED.with(|enabled| enabled.set(false));
    }

    pub(in crate::cache_action_stage::compression) fn leader_was_already_reaped() -> bool {
        LEADER_ALREADY_REAPED.with(Cell::get)
    }

    pub(in crate::cache_action_stage::compression) fn observe_and_suppress(group: u32) -> bool {
        use rustix::process::{Pid, WaitOptions, waitpid};

        if !ENABLED.with(Cell::get) {
            return false;
        }

        let Ok(raw_pid) = i32::try_from(group) else {
            return false;
        };
        let Some(pid) = Pid::from_raw(raw_pid) else {
            return false;
        };
        let observed = waitpid(Some(pid), WaitOptions::NOHANG);
        LEADER_ALREADY_REAPED.with(|already_reaped| {
            already_reaped.set(matches!(observed, Err(rustix::io::Errno::CHILD)));
        });
        matches!(observed, Err(rustix::io::Errno::CHILD) | Ok(Some(_)))
    }
}
