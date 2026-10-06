use std::io::{self, Read};

use rustix::fs::{OFlags, fcntl_getfl, fcntl_setfl};

const READ_CHUNK: usize = 8 * 1024;
const DRAIN_BUDGET: usize = 64 * 1024;

pub(super) struct Capture<R> {
    reader: Option<R>,
    pub(super) bytes: Vec<u8>,
    pub(super) cap: usize,
    pub(super) overflowed: bool,
}

impl<R: Read> Capture<R> {
    pub(super) fn new(reader: R, bytes: Vec<u8>, cap: usize) -> Self {
        Self {
            reader: Some(reader),
            bytes,
            cap,
            overflowed: false,
        }
    }

    pub(super) fn drain(&mut self) -> Result<(), String> {
        let mut buffer = [0; READ_CHUNK];
        let mut drained = 0;
        while drained < DRAIN_BUDGET {
            let Some(reader) = self.reader.as_mut() else {
                return Ok(());
            };
            let available = self.cap.saturating_sub(self.bytes.len());
            let limit = if self.overflowed {
                buffer.len().min(DRAIN_BUDGET - drained)
            } else {
                available
                    .min(buffer.len())
                    .max(1)
                    .min(DRAIN_BUDGET - drained)
            };
            match reader.read(&mut buffer[..limit]) {
                Ok(0) => {
                    self.reader = None;
                    return Ok(());
                }
                Ok(count) if self.overflowed || self.bytes.len() == self.cap => {
                    drained += count;
                    self.overflowed = true;
                }
                Ok(count) => {
                    drained += count;
                    self.bytes.extend_from_slice(&buffer[..count]);
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(()),
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => {
                    self.reader = None;
                    return Err(format!("output read failed ({error})"));
                }
            }
        }
        Ok(())
    }

    pub(super) fn is_closed(&self) -> bool {
        self.reader.is_none()
    }
}

pub(super) fn set_nonblocking<R: std::os::fd::AsFd>(reader: &R) -> Result<(), String> {
    let flags = fcntl_getfl(reader).map_err(|error| error.to_string())?;
    fcntl_setfl(reader, flags | OFlags::NONBLOCK).map_err(|error| error.to_string())
}

pub(super) fn reserve_capture(cap: usize) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(cap)
        .map_err(|error| format!("capture allocation failed ({error})"))?;
    Ok(bytes)
}
