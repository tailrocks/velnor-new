//! Bounded threaded pipe draining used on platforms without nonblocking pipes.

#[cfg(any(not(unix), test))]
use super::process;
#[cfg(any(not(unix), test))]
use std::io::{self, Read};
#[cfg(any(not(unix), test))]
use std::sync::Mutex;

#[cfg(any(not(unix), test))]
use std::process::{Child, ChildStderr, ChildStdout};
#[cfg(any(not(unix), test))]
use std::sync::Arc;
#[cfg(any(not(unix), test))]
use std::thread::{self, JoinHandle};
#[cfg(any(not(unix), test))]
use std::time::{Duration, Instant};

#[cfg(any(not(unix), test))]
type SharedOutput = Arc<Mutex<Vec<u8>>>;

#[cfg(any(not(unix), test))]
type SharedStreams = (SharedOutput, SharedOutput);

#[cfg(any(not(unix), test))]
struct OutputReaders {
    stdout: Option<JoinHandle<io::Result<()>>>,
    stderr: Option<JoinHandle<io::Result<()>>>,
    streams: SharedStreams,
}

#[cfg(any(not(unix), test))]
pub(super) fn run_threaded_bounded(
    child: &mut Child,
    limits: process::RunLimits,
) -> io::Result<process::LiveOutput> {
    let (stdout, stderr) = match process::take_output(child) {
        Ok(streams) => streams,
        Err(error) => return Err(fail_reader_start(child, &error, limits.cleanup)),
    };
    let mut readers = spawn_output_readers(child, stdout, stderr, limits.cleanup)?;
    let started = Instant::now();
    let mut status = None;
    let mut exited_at = None;
    loop {
        if status.is_none() {
            match child.try_wait() {
                Ok(result) => {
                    status = result;
                    if status.is_some() {
                        exited_at = Some(Instant::now());
                    }
                }
                Err(error) => {
                    return Err(cleanup_reader_error(
                        child,
                        &mut readers,
                        &error,
                        limits.cleanup,
                    ));
                }
            }
        }
        if let Some(status) = status {
            if readers_finished(&readers) {
                join_finished_readers(&mut readers)?;
                return Ok(process::LiveOutput {
                    status: Some(status),
                    streams: reader_output(&readers)?,
                });
            }
            if exited_at.is_some_and(|time| time.elapsed() >= limits.output_drain) {
                return finish_readers_after_exit(child, status, readers, limits.cleanup);
            }
        } else if started.elapsed() >= limits.runtime {
            return finish_readers_after_timeout(child, readers, limits.cleanup);
        }
        thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(any(not(unix), test))]
fn spawn_output_readers(
    child: &mut Child,
    stdout: ChildStdout,
    stderr: ChildStderr,
    cleanup: Duration,
) -> io::Result<OutputReaders> {
    let streams = (
        Arc::new(Mutex::new(Vec::new())),
        Arc::new(Mutex::new(Vec::new())),
    );
    let out = Arc::clone(&streams.0);
    let stdout = match thread::Builder::new()
        .name("cargo-deny-stdout".to_owned())
        .spawn(move || capture_output(stdout, &out))
    {
        Ok(reader) => reader,
        Err(error) => return Err(fail_reader_start(child, &error, cleanup)),
    };
    let err = Arc::clone(&streams.1);
    let stderr = match thread::Builder::new()
        .name("cargo-deny-stderr".to_owned())
        .spawn(move || capture_output(stderr, &err))
    {
        Ok(reader) => reader,
        Err(error) => {
            let mut readers = OutputReaders {
                stdout: Some(stdout),
                stderr: None,
                streams,
            };
            return Err(cleanup_reader_error(child, &mut readers, &error, cleanup));
        }
    };
    Ok(OutputReaders {
        stdout: Some(stdout),
        stderr: Some(stderr),
        streams,
    })
}

#[cfg(any(not(unix), test))]
fn capture_output(mut reader: impl Read, output: &Mutex<Vec<u8>>) -> io::Result<()> {
    let mut buffer = [0_u8; 8192];
    let mut interrupted = 0;
    loop {
        match reader.read(&mut buffer) {
            Ok(0) => return Ok(()),
            Ok(count) => {
                let mut output = output
                    .lock()
                    .map_err(|_| io::Error::other("cargo-deny output buffer lock poisoned"))?;
                process::append_tail(&mut output, &buffer[..count]);
                interrupted = 0;
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {
                interrupted += 1;
                if interrupted >= process::INTERRUPTED_READ_LIMIT {
                    return Err(io::Error::other(
                        "cargo-deny output reader repeatedly interrupted",
                    ));
                }
            }
            Err(error) => return Err(error),
        }
    }
}

#[cfg(any(not(unix), test))]
fn readers_finished(readers: &OutputReaders) -> bool {
    [readers.stdout.as_ref(), readers.stderr.as_ref()]
        .into_iter()
        .flatten()
        .all(JoinHandle::is_finished)
}

#[cfg(any(not(unix), test))]
fn join_finished_readers(readers: &mut OutputReaders) -> io::Result<()> {
    let mut failures = Vec::new();
    for handle in [readers.stdout.take(), readers.stderr.take()]
        .into_iter()
        .flatten()
    {
        match handle.join() {
            Ok(Ok(())) => {}
            Ok(Err(error)) => failures.push(error.to_string()),
            Err(_) => failures.push("cargo-deny output reader panicked".to_owned()),
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(io::Error::other(failures.join("; ")))
    }
}

#[cfg(any(not(unix), test))]
fn wait_readers(readers: &mut OutputReaders, deadline: Instant) -> io::Result<bool> {
    while !readers_finished(readers) && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(20));
    }
    if readers_finished(readers) {
        join_finished_readers(readers)?;
        Ok(true)
    } else {
        Ok(false)
    }
}

#[cfg(any(not(unix), test))]
fn reader_output(readers: &OutputReaders) -> io::Result<(Vec<u8>, Vec<u8>)> {
    Ok((snapshot(&readers.streams.0)?, snapshot(&readers.streams.1)?))
}

#[cfg(any(not(unix), test))]
fn snapshot(output: &Mutex<Vec<u8>>) -> io::Result<Vec<u8>> {
    output
        .lock()
        .map(|bytes| bytes.clone())
        .map_err(|_| io::Error::other("cargo-deny output buffer lock poisoned"))
}

#[cfg(any(not(unix), test))]
fn finish_readers_after_timeout(
    child: &mut Child,
    mut readers: OutputReaders,
    cleanup: Duration,
) -> io::Result<process::LiveOutput> {
    let deadline = Instant::now() + cleanup;
    if let Err(error) = process::terminate_process_tree(child, deadline) {
        return Err(reader_cleanup_error(&mut readers, &error, cleanup));
    }
    if process::wait_until(child, deadline)?.is_none() {
        return Err(reader_cleanup_error(
            &mut readers,
            &io::Error::other("timed out during process cleanup"),
            cleanup,
        ));
    }
    if wait_readers(&mut readers, deadline)? {
        return Ok(process::LiveOutput {
            status: None,
            streams: reader_output(&readers)?,
        });
    }
    Ok(process::LiveOutput {
        status: None,
        streams: reader_output(&readers)?,
    })
}

#[cfg(any(not(unix), test))]
fn finish_readers_after_exit(
    child: &mut Child,
    status: std::process::ExitStatus,
    mut readers: OutputReaders,
    cleanup: Duration,
) -> io::Result<process::LiveOutput> {
    let deadline = Instant::now() + cleanup;
    if let Err(error) = process::terminate_process_tree(child, deadline) {
        return Err(reader_cleanup_error(&mut readers, &error, cleanup));
    }
    if !wait_readers(&mut readers, deadline)? {
        return Err(io::Error::other(
            "output readers remained open after process-tree cleanup",
        ));
    }
    Ok(process::LiveOutput {
        status: Some(status),
        streams: reader_output(&readers)?,
    })
}

#[cfg(any(not(unix), test))]
fn cleanup_reader_error(
    child: &mut Child,
    readers: &mut OutputReaders,
    source: &io::Error,
    cleanup: Duration,
) -> io::Error {
    let deadline = Instant::now() + cleanup;
    let mut detail = source.to_string();
    if let Err(error) = process::terminate_process_tree(child, deadline) {
        process::append_error(&mut detail, "; cleanup failed: ", &error);
    }
    match process::wait_until(child, deadline) {
        Ok(Some(_)) => {}
        Ok(None) => detail.push_str("; child cleanup timed out"),
        Err(error) => process::append_error(&mut detail, "; child wait failed: ", &error),
    }
    append_reader_drain(&mut detail, readers, deadline);
    io::Error::other(detail)
}

#[cfg(any(not(unix), test))]
fn reader_cleanup_error(
    readers: &mut OutputReaders,
    source: &io::Error,
    cleanup: Duration,
) -> io::Error {
    let mut detail = source.to_string();
    append_reader_drain(&mut detail, readers, Instant::now() + cleanup);
    io::Error::other(detail)
}

#[cfg(any(not(unix), test))]
fn append_reader_drain(detail: &mut String, readers: &mut OutputReaders, deadline: Instant) {
    match wait_readers(readers, deadline) {
        Ok(true) => {}
        Ok(false) => detail.push_str("; output reader cleanup timed out"),
        Err(error) => process::append_error(detail, "; output drain failed: ", &error),
    }
}

#[cfg(any(not(unix), test))]
fn fail_reader_start(
    child: &mut std::process::Child,
    source: &io::Error,
    cleanup: std::time::Duration,
) -> io::Error {
    process::cleanup_failed_run(
        child,
        &mut None,
        &mut None,
        &mut (Vec::new(), Vec::new()),
        source,
        cleanup,
        false,
    )
}

#[cfg(test)]
#[path = "advisories_reader_tests.rs"]
mod tests;
