// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::SeekFrom;

use velnor_actions_orchestrator::{FilesystemMetadata, GenerationFile, OrchestratorError};

#[derive(Debug)]
pub(super) struct ForwardFile {
    inner: Box<dyn GenerationFile>,
}

impl ForwardFile {
    pub(super) fn new(inner: Box<dyn GenerationFile>) -> Self {
        Self { inner }
    }
}

impl std::io::Read for ForwardFile {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        self.inner.read(buffer)
    }
}

impl std::io::Write for ForwardFile {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        self.inner.write(buffer)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

impl std::io::Seek for ForwardFile {
    fn seek(&mut self, position: SeekFrom) -> std::io::Result<u64> {
        self.inner.seek(position)
    }
}

impl GenerationFile for ForwardFile {
    fn metadata(&self) -> Result<FilesystemMetadata, OrchestratorError> {
        self.inner.metadata()
    }

    fn set_mode(&self, mode: u32) -> Result<(), OrchestratorError> {
        self.inner.set_mode(mode)
    }
}
