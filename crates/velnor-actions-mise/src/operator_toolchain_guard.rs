// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};

use crate::MiseError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperatorToolchainGuard {
    executable: PathBuf,
}

impl OperatorToolchainGuard {
    pub fn new(path: impl AsRef<Path>) -> Result<Self, MiseError> {
        let path = path.as_ref();
        if !path.is_absolute() {
            return Err(MiseError::operator_toolchain_guard(
                "operator toolchain guard requires an absolute path",
            ));
        }
        let canonical = path.canonicalize().map_err(|_| {
            MiseError::operator_toolchain_guard(
                "operator toolchain guard requires an executable regular file",
            )
        })?;
        if canonical != path {
            return Err(MiseError::operator_toolchain_guard(
                "operator toolchain guard requires a canonical path",
            ));
        }
        let metadata = std::fs::symlink_metadata(path).map_err(|_| {
            MiseError::operator_toolchain_guard(
                "operator toolchain guard requires an executable regular file",
            )
        })?;
        if !metadata.is_file() || metadata.file_type().is_symlink() || !is_executable(&metadata) {
            return Err(MiseError::operator_toolchain_guard(
                "operator toolchain guard requires an executable regular file",
            ));
        }
        Ok(Self {
            executable: canonical,
        })
    }

    #[must_use]
    pub fn executable(&self) -> &Path {
        &self.executable
    }
}

#[cfg(unix)]
fn is_executable(metadata: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;

    metadata.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn is_executable(_metadata: &std::fs::Metadata) -> bool {
    true
}
