//! Existing bounded subprocess polling, with explicit child reaping on errors.

use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use super::output::{read_capped, signal_of};
use super::{CancelHandle, IsolatedCommand, ProcessOutput};
use crate::MiseError;

type Reader = JoinHandle<(Vec<u8>, bool)>;

pub(super) struct Outcome {
    pub(super) result: Result<ProcessOutput, MiseError>,
    pub(super) safe_to_cleanup: bool,
}

pub(super) fn run(
    owner: &IsolatedCommand,
    command: Command,
    cap: usize,
    timeout: Duration,
    cancel: &CancelHandle,
) -> Outcome {
    let mut safe_to_cleanup = true;
    let result = wait(owner, command, cap, timeout, cancel, &mut safe_to_cleanup);
    Outcome {
        result,
        safe_to_cleanup,
    }
}

fn wait(
    owner: &IsolatedCommand,
    mut command: Command,
    cap: usize,
    timeout: Duration,
    cancel: &CancelHandle,
    safe_to_cleanup: &mut bool,
) -> Result<ProcessOutput, MiseError> {
    let program = owner.program.to_string_lossy().into_owned();
    let fail = |message: &str| MiseError::SpawnFailed {
        program: program.clone(),
        message: message.to_owned(),
    };
    if cancel.is_cancelled() {
        return Err(fail(super::SPAWN_CANCELLED_MESSAGE));
    }
    let deadline = Instant::now()
        .checked_add(timeout)
        .ok_or_else(|| fail("timeout_out_of_range"))?;
    if let Some(executable) = &owner.resolved_git {
        if command.get_program() != executable.path().as_os_str() {
            return Err(fail("git_native_program_mismatch"));
        }
        executable.verify()?;
        if Instant::now() >= deadline {
            return Err(fail(&format!(
                "{}{}",
                super::SPAWN_TIMEOUT_MESSAGE_PREFIX,
                timeout.as_secs()
            )));
        }
    }
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| fail(&error.to_string()))?;
    *safe_to_cleanup = false;
    let (stdout, stderr) = (child.stdout.take(), child.stderr.take());
    let out_reader = std::thread::spawn(move || read_capped(stdout, cap));
    let err_reader = std::thread::spawn(move || read_capped(stderr, cap));
    loop {
        if cancel.is_cancelled() {
            stop(&mut child).map_err(|error| fail(&error.to_string()))?;
            *safe_to_cleanup = true;
            drop((out_reader, err_reader));
            return Err(fail(super::SPAWN_CANCELLED_MESSAGE));
        }
        let status = match child.try_wait() {
            Ok(status) => status,
            Err(error) => {
                let cleanup = stop(&mut child);
                *safe_to_cleanup = cleanup.is_ok();
                drop((out_reader, err_reader));
                return Err(fail(&format!("{error}; child_reap={cleanup:?}")));
            }
        };
        if let Some(status) = status {
            *safe_to_cleanup = true;
            return output(status, out_reader, err_reader, cap, &fail);
        }
        if Instant::now() >= deadline {
            stop(&mut child).map_err(|error| fail(&error.to_string()))?;
            *safe_to_cleanup = true;
            drop((out_reader, err_reader));
            return Err(fail(&format!(
                "{}{}",
                super::SPAWN_TIMEOUT_MESSAGE_PREFIX,
                timeout.as_secs()
            )));
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn stop(child: &mut Child) -> std::io::Result<()> {
    let killed = child.kill();
    let waited = child.wait();
    match (killed, waited) {
        (_, Ok(_)) => Ok(()),
        (Ok(()), Err(error)) => Err(error),
        (Err(kill), Err(wait)) => Err(std::io::Error::other(format!(
            "child_kill={kill}; child_wait={wait}"
        ))),
    }
}

fn output(
    status: ExitStatus,
    out_reader: Reader,
    err_reader: Reader,
    cap: usize,
    fail: &impl Fn(&str) -> MiseError,
) -> Result<ProcessOutput, MiseError> {
    let (out, out_capped) = out_reader
        .join()
        .map_err(|_| fail("reader_panicked:stdout"))?;
    let (err, err_capped) = err_reader
        .join()
        .map_err(|_| fail("reader_panicked:stderr"))?;
    if out_capped || err_capped {
        let stream = if out_capped { "stdout" } else { "stderr" };
        return Err(fail(&format!("{stream}_limit_exceeded:{cap}")));
    }
    Ok(ProcessOutput {
        stdout: out,
        stderr: err,
        code: status.code(),
        signal: signal_of(status),
        success: status.success(),
    })
}
