//! Bounded stdout sinks for the single process-group polling authority.
use super::{IsolatedCommand, ProcessFileOutput};
use crate::{CheckDeadline, MiseError};
#[cfg(unix)]
use std::io::Write;

impl IsolatedCommand {
    /// Stream stdout into a caller-owned regular file under one absolute deadline.
    ///
    /// Writes start at the current file position. The caller must exclusively own
    /// the file, authenticate its identity and discard partial bytes on error.
    /// Stderr has a separate in-memory cap; no stdout-sized allocation is made.
    /// # Errors
    /// Returns `SpawnFailed` on unsupported platforms, nonregular files, write,
    /// output-limit, deadline or process cleanup failure.
    pub fn run_stdout_to_file_until(
        &self,
        output: &mut std::fs::File,
        stdout_cap: u64,
        stderr_cap: usize,
        deadline: CheckDeadline,
    ) -> Result<ProcessFileOutput, MiseError> {
        #[cfg(unix)]
        {
            if !output.metadata().is_ok_and(|metadata| metadata.is_file()) {
                return Err(MiseError::SpawnFailed {
                    program: self.program.to_string_lossy().into_owned(),
                    message: "stdout_sink_requires_regular_file".to_owned(),
                });
            }
            let mut streams = Streams {
                stdout: Sink::File(output),
                stdout_cap,
                stdout_bytes: 0,
                stderr_cap,
            };
            let result = self.run_streams(
                &mut streams,
                deadline,
                &super::CancelHandle::new(),
                super::SPAWN_DEADLINE_MESSAGE,
            )?;
            Ok(ProcessFileOutput {
                stdout_bytes: streams.stdout_bytes,
                stderr: result.stderr,
                code: result.code,
                signal: result.signal,
                success: result.success,
            })
        }
        #[cfg(not(unix))]
        {
            let _ = (output, stdout_cap, stderr_cap, deadline);
            Err(MiseError::SpawnFailed {
                program: self.program.to_string_lossy().into_owned(),
                message: "cancellable_process_groups_require_unix".to_owned(),
            })
        }
    }
}

#[cfg(unix)]
enum Sink<'a> {
    Memory(Vec<u8>),
    File(&'a mut std::fs::File),
}

#[cfg(unix)]
pub(super) struct Streams<'a> {
    stdout: Sink<'a>,
    stdout_cap: u64,
    stdout_bytes: u64,
    pub(super) stderr_cap: usize,
}

#[cfg(unix)]
impl Streams<'_> {
    pub(super) fn memory(cap: usize) -> Self {
        Self {
            stdout: Sink::Memory(Vec::with_capacity(cap.min(64 * 1024))),
            stdout_cap: u64::try_from(cap).unwrap_or(u64::MAX),
            stdout_bytes: 0,
            stderr_cap: cap,
        }
    }

    pub(super) fn into_memory(self) -> Vec<u8> {
        match self.stdout {
            Sink::Memory(bytes) => bytes,
            Sink::File(_) => Vec::new(),
        }
    }

    pub(super) fn poll_stdout<Fd: std::os::fd::AsFd>(
        &mut self,
        pipe: &mut Option<Fd>,
    ) -> Result<bool, String> {
        poll_chunk(pipe, |bytes| {
            let next = self
                .stdout_bytes
                .checked_add(u64::try_from(bytes.len()).map_err(|error| error.to_string())?)
                .ok_or_else(|| "output_limit_exceeded".to_owned())?;
            if next > self.stdout_cap {
                return Err(format!("stdout_limit_exceeded:{}", self.stdout_cap));
            }
            match &mut self.stdout {
                Sink::Memory(output) => output.extend_from_slice(bytes),
                Sink::File(output) => output
                    .write_all(bytes)
                    .map_err(|error| format!("stdout_sink_write_failed:{error}"))?,
            }
            self.stdout_bytes = next;
            Ok(())
        })
    }
}

#[cfg(unix)]
pub(super) fn poll_stderr<Fd: std::os::fd::AsFd>(
    pipe: &mut Option<Fd>,
    output: &mut Vec<u8>,
    cap: usize,
) -> Result<bool, String> {
    poll_chunk(pipe, |bytes| {
        let next = output
            .len()
            .checked_add(bytes.len())
            .ok_or_else(|| "output_limit_exceeded".to_owned())?;
        if next > cap {
            return Err(format!("stderr_limit_exceeded:{cap}"));
        }
        output.extend_from_slice(bytes);
        Ok(())
    })
}

/// One chunk per turn keeps continuously readable pipes from starving the
/// shared deadline/cancellation checkpoint or the other stream.
#[cfg(unix)]
fn poll_chunk<Fd: std::os::fd::AsFd>(
    pipe: &mut Option<Fd>,
    mut accept: impl FnMut(&[u8]) -> Result<(), String>,
) -> Result<bool, String> {
    let Some(reader) = pipe.as_ref() else {
        return Ok(false);
    };
    let mut buffer = [0_u8; 8192];
    match rustix::io::read(reader, &mut buffer[..]) {
        Ok(0) => {
            pipe.take();
            Ok(false)
        }
        Ok(count) => {
            accept(&buffer[..count])?;
            Ok(true)
        }
        Err(error) if error == rustix::io::Errno::INTR || error == rustix::io::Errno::AGAIN => {
            Ok(false)
        }
        Err(error) => Err(error.to_string()),
    }
}
