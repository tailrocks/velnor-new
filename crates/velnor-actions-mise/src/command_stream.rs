// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{IsolatedCommand, ProcessFileOutput};
use crate::{CheckDeadline, MiseError};
#[cfg(unix)]
use std::io::Write;

impl IsolatedCommand {
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
                stderr: Vec::with_capacity(stderr_cap.min(64 * 1024)),
                progress: None,
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
    stderr: Vec<u8>,
    progress: Option<super::progress::ProgressSink<'a>>,
}

#[cfg(unix)]
impl<'a> Streams<'a> {
    pub(super) fn file(output: &'a mut std::fs::File, stdout_cap: u64, stderr_cap: usize) -> Self {
        Self {
            stdout: Sink::File(output),
            stdout_cap,
            stdout_bytes: 0,
            stderr_cap,
            stderr: Vec::with_capacity(stderr_cap.min(64 * 1024)),
            progress: None,
        }
    }

    pub(super) const fn stdout_bytes(&self) -> u64 {
        self.stdout_bytes
    }

    pub(super) fn memory(cap: usize) -> Self {
        Self {
            stdout: Sink::Memory(Vec::with_capacity(cap.min(64 * 1024))),
            stdout_cap: u64::try_from(cap).unwrap_or(u64::MAX),
            stdout_bytes: 0,
            stderr_cap: cap,
            stderr: Vec::with_capacity(cap.min(64 * 1024)),
            progress: None,
        }
    }

    pub(super) fn observe(mut self, output: &'a mut dyn Write, deadline: CheckDeadline) -> Self {
        self.progress = Some(super::progress::ProgressSink::new(output, deadline));
        self
    }

    pub(super) fn into_memory(self) -> Vec<u8> {
        match self.stdout {
            Sink::Memory(bytes) => bytes,
            Sink::File(_) => Vec::new(),
        }
    }

    pub(super) fn into_prefixes(self) -> (Vec<u8>, Vec<u8>) {
        let stdout = match self.stdout {
            Sink::Memory(bytes) => bytes,
            Sink::File(_) => Vec::new(),
        };
        (stdout, self.stderr)
    }

    pub(super) fn take_stderr(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.stderr)
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
            let available = self.stdout_cap.saturating_sub(self.stdout_bytes);
            let accepted = bytes
                .len()
                .min(usize::try_from(available).unwrap_or(usize::MAX));
            let prefix = &bytes[..accepted];
            match &mut self.stdout {
                Sink::Memory(output) => output.extend_from_slice(prefix),
                Sink::File(output) => output
                    .write_all(prefix)
                    .map_err(|error| format!("stdout_sink_write_failed:{error}"))?,
            }
            self.stdout_bytes = self
                .stdout_bytes
                .checked_add(u64::try_from(accepted).map_err(|error| error.to_string())?)
                .ok_or_else(|| "output_limit_exceeded".to_owned())?;
            if let Some(progress) = &mut self.progress {
                progress.chunk("stdout", prefix)?;
            }
            if accepted < bytes.len() || next > self.stdout_cap {
                return Err(format!("stdout_limit_exceeded:{}", self.stdout_cap));
            }
            Ok(())
        })
    }
}

#[cfg(unix)]
impl Streams<'_> {
    pub(super) fn poll_stderr<Fd: std::os::fd::AsFd>(
        &mut self,
        pipe: &mut Option<Fd>,
    ) -> Result<bool, String> {
        poll_chunk(pipe, |bytes| {
            let next = self
                .stderr
                .len()
                .checked_add(bytes.len())
                .ok_or_else(|| "output_limit_exceeded".to_owned())?;
            let accepted = bytes
                .len()
                .min(self.stderr_cap.saturating_sub(self.stderr.len()));
            let prefix = &bytes[..accepted];
            self.stderr.extend_from_slice(prefix);
            if let Some(progress) = &mut self.progress {
                progress.chunk("stderr", prefix)?;
            }
            if accepted < bytes.len() || next > self.stderr_cap {
                return Err(format!("stderr_limit_exceeded:{}", self.stderr_cap));
            }
            Ok(())
        })
    }
}

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
