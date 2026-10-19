//! Explicit, bounded diagnostic observation of credential-free task output.
use super::IsolatedCommand;
use crate::{CheckDeadline, MiseError, ProcessOutput};
use std::io::Write;

impl IsolatedCommand {
    /// Observe escaped stdout and stderr while retaining ordinary capture limits.
    ///
    /// Callers MUST opt in only for credential-free tasks. Every source chunk is
    /// escaped and prefixed; raw output is never written to the observer. A fixed
    /// 256KiB total encoded budget includes prefixes and the truncation marker.
    /// The caller must supply a responsive diagnostic sink: synchronous OS writes
    /// cannot be physically preempted, just like the existing owned-file sink.
    /// This reuses the original deadline, polling and process-group cleanup.
    /// # Errors
    /// Fails on the ordinary process limits, deadline, cleanup or observer writes.
    pub fn run_until_with_progress(
        &self,
        cap: usize,
        deadline: CheckDeadline,
        output: &mut dyn Write,
    ) -> Result<ProcessOutput, MiseError> {
        #[cfg(unix)]
        {
            let mut streams = super::stream::Streams::memory(cap).observe(output, deadline);
            let mut result = self.run_streams(
                &mut streams,
                deadline,
                &super::CancelHandle::new(),
                super::SPAWN_DEADLINE_MESSAGE,
            )?;
            result.stdout = streams.into_memory();
            Ok(result)
        }
        #[cfg(not(unix))]
        {
            let _ = output;
            self.run_until(cap, deadline)
        }
    }
}

#[cfg(unix)]
const MAX_PROGRESS_BYTES: usize = 256 * 1024;
#[cfg(unix)]
const TRUNCATED: &[u8] =
    b"velnor-progress: diagnostic limit reached; further task output omitted\n";

#[cfg(unix)]
pub(super) struct ProgressSink<'a> {
    output: &'a mut dyn Write,
    deadline: CheckDeadline,
    written: usize,
    truncated: bool,
}

#[cfg(unix)]
impl<'a> ProgressSink<'a> {
    pub(super) fn new(output: &'a mut dyn Write, deadline: CheckDeadline) -> Self {
        Self {
            output,
            deadline,
            written: 0,
            truncated: false,
        }
    }

    pub(super) fn chunk(&mut self, stream: &str, bytes: &[u8]) -> Result<(), String> {
        if self.truncated {
            return Ok(());
        }
        let mut encoded = format!("velnor-progress {stream}: ").into_bytes();
        for &byte in bytes {
            match byte {
                // The runner's legacy parser searches for ##[ ANYWHERE in a
                // line; a prefix alone does not prevent workflow commands.
                b' '..=b'~' if byte != b'#' && byte != b'\\' => encoded.push(byte),
                _ => {
                    const HEX: &[u8; 16] = b"0123456789abcdef";
                    encoded.extend_from_slice(&[
                        b'\\',
                        b'x',
                        HEX[usize::from(byte >> 4)],
                        HEX[usize::from(byte & 15)],
                    ]);
                }
            }
        }
        encoded.push(b'\n');
        if encoded.len() > MAX_PROGRESS_BYTES - TRUNCATED.len() - self.written {
            self.truncated = true;
            return self.emit(TRUNCATED);
        }
        self.emit(&encoded)
    }

    fn emit(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.deadline
            .remaining()
            .map_err(|_| super::SPAWN_DEADLINE_MESSAGE.to_owned())?;
        self.output
            .write_all(bytes)
            .map_err(|_| "progress_write_failed".to_owned())?;
        self.output
            .flush()
            .map_err(|_| "progress_flush_failed".to_owned())?;
        self.written += bytes.len();
        self.deadline
            .remaining()
            .map_err(|_| super::SPAWN_DEADLINE_MESSAGE.to_owned())?;
        Ok(())
    }
}

#[cfg(all(test, unix))]
#[path = "command_progress_tests.rs"]
mod tests;
