// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use rustix::fs::{Mode, OFlags, StatVfsMountFlags, fstatvfs, open};
use rustix::io::Errno;

use crate::exec::ExecError;

#[cfg(feature = "test-support")]
fn declared_read_only(directory: &Path) -> bool {
    std::env::var_os("VELNOR_TEST_READ_ONLY_DIRS")
        .is_some_and(|list| std::env::split_paths(&list).any(|declared| declared == directory))
}

#[cfg(not(feature = "test-support"))]
fn declared_read_only(_directory: &Path) -> bool {
    false
}

fn check(role: &str, directory: &Path) -> Result<(), ExecError> {
    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let descriptor = open(directory, flags, Mode::empty()).map_err(|error| match error {
        Errno::NOENT => ExecError::Invalid(format!(
            "mise policy: {role} {} is missing from the tools store",
            directory.display()
        )),
        other => ExecError::io(format!("open {}", directory.display()), other.into()),
    })?;
    let stats = fstatvfs(&descriptor)
        .map_err(|error| ExecError::io(format!("statvfs {}", directory.display()), error.into()))?;
    if stats.f_flag.contains(StatVfsMountFlags::RDONLY) || declared_read_only(directory) {
        return Ok(());
    }
    Err(ExecError::Invalid(format!(
        "mise policy: {role} {} is not on a read-only mount; the tools store must be mounted read-only",
        directory.display()
    )))
}

pub(super) fn require_read_only_store(directories: &[(&str, &Path)]) -> Result<(), ExecError> {
    directories
        .iter()
        .try_for_each(|(role, directory)| check(role, directory))
}
