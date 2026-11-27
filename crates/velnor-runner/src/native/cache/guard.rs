// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::{self, Read, Write};

use super::archive::{Cancelled, checkpoint};
use crate::exec::JobCancel;

pub(super) struct Watched<'a, R> {
    pub(super) inner: R,
    pub(super) cancel: Option<&'a JobCancel>,
}

impl<R: Read> Read for Watched<'_, R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if checkpoint(self.cancel).is_err() {
            return Err(io::Error::other(Cancelled));
        }
        self.inner.read(buffer)
    }
}

pub(super) struct Capped<W> {
    inner: W,
    left: u64,
    written: u64,
}

impl<W> Capped<W> {
    pub(super) fn new(inner: W, limit: u64) -> Self {
        Self {
            inner,
            left: limit,
            written: 0,
        }
    }

    pub(super) fn into_parts(self) -> (W, u64) {
        (self.inner, self.written)
    }
}

impl<W: Write> Write for Capped<W> {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let size = u64::try_from(buffer.len()).unwrap_or(u64::MAX);
        if size > self.left {
            return Err(io::Error::other(
                "the archive is over the 10GB limit, not saving cache.",
            ));
        }
        let written = self.inner.write(buffer)?;
        let count = u64::try_from(written).unwrap_or(u64::MAX);
        self.left -= count;
        self.written += count;
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}
