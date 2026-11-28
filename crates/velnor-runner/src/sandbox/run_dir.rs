// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs::DirBuilder;
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
use std::path::Path;

use anyhow::{Context, Result, ensure};
use rustix::fs::{CWD, Mode, OFlags, openat};

use super::dirs;
use super::layout::run_dir_of;
use super::tree::directory_flags;

pub(super) const TRUSTED_FILE: &str = "trusted.json";
pub(super) const BREACH_FILE: &str = "breach.json";

const PRIVATE: u32 = 0o700;
const GROUP_AND_OTHERS: u32 = 0o077;

#[cfg(target_os = "linux")]
fn memory_backed(path: &Path) -> Result<bool> {
    let stat = rustix::fs::statfs(path)
        .with_context(|| format!("inspect the file system of {}", path.display()))?;
    Ok(i128::from(stat.f_type) == 0x0102_1994)
}

#[cfg(not(target_os = "linux"))]
fn memory_backed(_path: &Path) -> Result<bool> {
    anyhow::bail!("the sandbox needs Linux")
}

fn private_tmpfs(path: &Path) -> Result<()> {
    let metadata =
        std::fs::symlink_metadata(path).with_context(|| format!("inspect {}", path.display()))?;
    ensure!(
        metadata.is_dir() && metadata.uid() == 0 && metadata.mode() & GROUP_AND_OTHERS == 0,
        "{} is not a root-only directory",
        path.display()
    );
    ensure!(
        memory_backed(path)?,
        "{} is not on a tmpfs; the trusted facts must never reach a disk",
        path.display()
    );
    Ok(())
}

pub(super) fn create(reservation: u64) -> Result<()> {
    let directory = run_dir_of(reservation);
    let parent = directory
        .parent()
        .context("the run directory has no parent")?;
    DirBuilder::new()
        .recursive(true)
        .mode(PRIVATE)
        .create(parent)
        .with_context(|| format!("create {}", parent.display()))?;
    std::fs::set_permissions(parent, std::fs::Permissions::from_mode(PRIVATE))
        .with_context(|| format!("close {}", parent.display()))?;
    private_tmpfs(parent)?;
    remove(reservation)?;
    DirBuilder::new()
        .mode(PRIVATE)
        .create(&directory)
        .with_context(|| format!("create {}", directory.display()))
}

pub(super) fn write(reservation: u64, name: &str, bytes: &[u8]) -> Result<()> {
    write_at(&run_dir_of(reservation), name, bytes)
}

pub(super) fn write_at(directory: &Path, name: &str, bytes: &[u8]) -> Result<()> {
    let parent = openat(CWD, directory, directory_flags(), Mode::empty())
        .with_context(|| format!("open {}", directory.display()))?;
    let descriptor = openat(
        &parent,
        name,
        OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::CLOEXEC,
        Mode::RUSR | Mode::WUSR,
    )
    .with_context(|| format!("create {name} in {}", directory.display()))?;
    std::fs::File::from(descriptor)
        .write_all(bytes)
        .with_context(|| format!("write {name} in {}", directory.display()))
}

pub(super) fn remove(reservation: u64) -> Result<()> {
    dirs::remove(&run_dir_of(reservation))
}
