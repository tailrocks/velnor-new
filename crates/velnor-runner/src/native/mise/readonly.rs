// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};

use rustix::fs::{OFlags, StatVfsMountFlags, fstatvfs};
use rustix::io::Errno;

use super::mounts::require_no_writable_mount_under;
use super::tools::{Tools, open_tools};
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

fn check(
    role: &str,
    directory: &Path,
    relative: &Path,
    held: Option<&Tools>,
) -> Result<PathBuf, ExecError> {
    let missing = || {
        ExecError::Invalid(format!(
            "mise policy: {role} {} is missing from the tools store",
            directory.display()
        ))
    };
    let held = held.ok_or_else(missing)?;
    let descriptor = held
        .open_below(relative, OFlags::RDONLY | OFlags::DIRECTORY)
        .map_err(|error| match error {
            Errno::NOENT => missing(),
            Errno::NOTDIR => ExecError::Invalid(format!(
                "mise policy: {role} {} is reached through a link; the tools store holds only real directories",
                directory.display()
            )),
            other => ExecError::io(format!("open {}", directory.display()), other.into()),
        })?;
    let stats = fstatvfs(&descriptor)
        .map_err(|error| ExecError::io(format!("statvfs {}", directory.display()), error.into()))?;
    let real = held.real.join(relative);
    if stats.f_flag.contains(StatVfsMountFlags::RDONLY) || declared_read_only(&real) {
        return Ok(real);
    }
    Err(ExecError::Invalid(format!(
        "mise policy: {role} {} is not on a read-only mount; the tools store must be mounted read-only",
        directory.display()
    )))
}

pub(super) async fn require_read_only_store(
    tools: &Path,
    directories: &[(&str, &Path)],
    first: bool,
) -> Result<(), ExecError> {
    let held = open_tools(tools)?;
    let mut roots = Vec::with_capacity(directories.len());
    for (role, directory) in directories {
        let relative = directory.strip_prefix(tools).map_err(|_| {
            ExecError::Invalid(format!(
                "mise policy: {role} {} is outside the tools store {}",
                directory.display(),
                tools.display()
            ))
        })?;
        roots.push((*role, check(role, directory, relative, held.as_ref())?));
    }
    require_no_writable_mount_under(&roots, first).await
}
