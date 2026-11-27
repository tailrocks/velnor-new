// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use rustix::fs::{Access, access};
use rustix::io::Errno;

use crate::exec::ExecError;

pub(super) fn require_read_only_store(data: &Path, store: &Path) -> Result<(), ExecError> {
    for directory in data.ancestors().take_while(|path| path.starts_with(store)) {
        match access(directory, Access::WRITE_OK) {
            Ok(()) => {
                return Err(ExecError::Invalid(format!(
                    "mise policy: {} is writable by this process; the tools store holding MISE_DATA_DIR {} must be read-only",
                    directory.display(),
                    data.display()
                )));
            }
            Err(Errno::ACCESS | Errno::ROFS | Errno::PERM) => {}
            Err(Errno::NOENT) if directory == data => {
                return Err(ExecError::Invalid(format!(
                    "mise policy: MISE_DATA_DIR {} is missing from the tools store",
                    data.display()
                )));
            }
            Err(error) => {
                return Err(ExecError::io(
                    format!("check {}", directory.display()),
                    error.into(),
                ));
            }
        }
    }
    Ok(())
}
