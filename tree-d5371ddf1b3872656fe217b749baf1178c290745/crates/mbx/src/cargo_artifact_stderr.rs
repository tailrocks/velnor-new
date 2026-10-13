use super::{CargoBoundStderr, CargoCommandBinding};
use serde::Serialize;
use std::io::{Read, Write};

/// Owning stderr observation; absence of diagnostics is meaningful only at EOF.
#[derive(Debug, Serialize)]
pub struct CargoStderrCapture {
    #[serde(skip_serializing)]
    bytes: Vec<u8>,
    byte_count: u64,
    eof: bool,
    #[serde(skip_serializing)]
    read_error: Option<String>,
    #[serde(skip_serializing)]
    forward_error: Option<String>,
    read_failed: bool,
    forward_failed: bool,
    truncated: bool,
    command: CargoCommandBinding,
}

impl CargoStderrCapture {
    pub fn read(stderr: CargoBoundStderr, forward: &mut impl Write) -> Self {
        Self::read_stream(stderr.stream, forward, stderr.binding)
    }

    pub(super) fn read_stream(
        mut stderr: impl Read,
        forward: &mut impl Write,
        command: CargoCommandBinding,
    ) -> Self {
        let mut capture = Self {
            bytes: Vec::new(),
            byte_count: 0,
            eof: false,
            read_error: None,
            forward_error: None,
            read_failed: false,
            forward_failed: false,
            truncated: false,
            command,
        };
        let mut buffer = [0; 8192];
        loop {
            let size = match stderr.read(&mut buffer) {
                Ok(0) => {
                    capture.eof = true;
                    break;
                }
                Ok(size) => size,
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) => {
                    capture.read_failed = true;
                    capture.read_error = Some(error.to_string());
                    break;
                }
            };
            capture.byte_count = match capture.byte_count.checked_add(size as u64) {
                Some(bytes) => bytes,
                None => {
                    capture.truncated = true;
                    u64::MAX
                }
            };
            let remaining = (16 * 1024 * 1024_usize).saturating_sub(capture.bytes.len());
            capture
                .bytes
                .extend_from_slice(&buffer[..size.min(remaining)]);
            capture.truncated |= size > remaining;
            if capture.forward_error.is_none() {
                if let Err(error) = forward.write_all(&buffer[..size]) {
                    capture.forward_failed = true;
                    capture.forward_error = Some(error.to_string());
                }
            }
        }
        capture
    }

    pub fn complete(&self) -> bool {
        self.eof && self.read_error.is_none() && self.forward_error.is_none() && !self.truncated
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    pub fn output_failed(&self) -> bool {
        self.read_failed || self.forward_failed
    }
    pub(super) fn binding(&self) -> &CargoCommandBinding {
        &self.command
    }
}
