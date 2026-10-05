//! Deadline checks for compressed archive reads.
use std::io::{self, Read, Seek, SeekFrom};

use flate2::read::GzDecoder;
use lzma_rust2::XzReader;

use crate::OrchestratorError;
use crate::internal::internal;
use velnor_actions_mise::CheckDeadline;

use super::XZ_MEMORY_LIMIT_KIB;

pub(super) fn gzip_reader<R: Read>(
    source: R,
    deadline: CheckDeadline,
) -> DeadlineIo<GzDecoder<DeadlineIo<R>>> {
    DeadlineIo::new(GzDecoder::new(DeadlineIo::new(source, deadline)), deadline)
}

pub(super) fn xz_reader<R: Read>(
    source: R,
    deadline: CheckDeadline,
) -> DeadlineIo<XzReader<DeadlineIo<R>>> {
    DeadlineIo::new(
        XzReader::new_mem_limit(
            DeadlineIo::new(source, deadline),
            false,
            XZ_MEMORY_LIMIT_KIB,
        ),
        deadline,
    )
}

pub(super) fn check_deadline(deadline: CheckDeadline) -> Result<(), OrchestratorError> {
    deadline
        .remaining()
        .map(|_| ())
        .map_err(|error| internal(&error.to_string()))
}

pub(super) fn archive_error(problem: &str) -> OrchestratorError {
    internal(&format!("tool_archive:{problem}"))
}

pub(super) struct DeadlineIo<R> {
    inner: R,
    deadline: CheckDeadline,
}

impl<R> DeadlineIo<R> {
    pub(super) fn new(inner: R, deadline: CheckDeadline) -> Self {
        Self { inner, deadline }
    }

    fn checkpoint(&self) -> io::Result<()> {
        self.deadline
            .remaining()
            .map(|_| ())
            .map_err(|error| io::Error::new(io::ErrorKind::TimedOut, error.to_string()))
    }
}

impl<R: Read> Read for DeadlineIo<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.checkpoint()?;
        let count = self.inner.read(buffer)?;
        self.checkpoint()?;
        Ok(count)
    }
}

impl<R: Seek> Seek for DeadlineIo<R> {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.checkpoint()?;
        let position = self.inner.seek(position)?;
        self.checkpoint()?;
        Ok(position)
    }
}
