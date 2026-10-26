// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::IsolatedCommand;
use crate::{CheckDeadline, MiseError, ProcessOutput};
use std::io::Write;

impl IsolatedCommand {
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
