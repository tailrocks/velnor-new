// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::CString;
use std::fs::{DirBuilder, File};
use std::os::unix::fs::{DirBuilderExt, symlink};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use rustix::fs::{StatVfsMountFlags, statvfs};
use rustix::mount::{MountFlags, UnmountFlags, mount, mount_bind, mount_remount, unmount};
use rustix::process::pivot_root;

use super::{Layout, MountOp};

const OLD_ROOT: &str = ".pivot";
const ROOT_BYTES: u64 = 4 * 1024 * 1024;
const PUBLIC: u32 = 0o755;
const CARRIED: [(StatVfsMountFlags, MountFlags); 7] = [
    (StatVfsMountFlags::NOATIME, MountFlags::NOATIME),
    (StatVfsMountFlags::NODIRATIME, MountFlags::NODIRATIME),
    (StatVfsMountFlags::RELATIME, MountFlags::RELATIME),
    (StatVfsMountFlags::NOEXEC, MountFlags::NOEXEC),
    (StatVfsMountFlags::NODEV, MountFlags::NODEV),
    (StatVfsMountFlags::NOSUID, MountFlags::NOSUID),
    (StatVfsMountFlags::RDONLY, MountFlags::RDONLY),
];

fn inside(root: &Path, path: &Path) -> PathBuf {
    root.join(path.strip_prefix("/").unwrap_or(path))
}

fn directory(path: &Path) -> Result<()> {
    DirBuilder::new()
        .recursive(true)
        .mode(PUBLIC)
        .create(path)
        .with_context(|| format!("create {}", path.display()))
}

fn mountpoint(source: &Path, target: &Path) -> Result<()> {
    if std::fs::metadata(source)
        .with_context(|| format!("inspect {}", source.display()))?
        .is_dir()
    {
        return directory(target);
    }
    if let Some(parent) = target.parent() {
        directory(parent)?;
    }
    File::create(target)
        .map(drop)
        .with_context(|| format!("create {}", target.display()))
}

fn carried(path: &Path) -> Result<MountFlags> {
    let present = statvfs(path)
        .with_context(|| format!("read the mount flags of {}", path.display()))?
        .f_flag;
    Ok(CARRIED
        .iter()
        .filter(|(stat, _)| present.contains(*stat))
        .fold(MountFlags::empty(), |flags, (_, mount)| flags | *mount))
}

fn bind(source: &Path, target: &Path) -> Result<()> {
    mountpoint(source, target)?;
    mount_bind(source, target)
        .with_context(|| format!("bind {} onto {}", source.display(), target.display()))
}

fn restrict(target: &Path, extra: MountFlags) -> Result<()> {
    let flags =
        MountFlags::BIND | MountFlags::NOSUID | MountFlags::NODEV | extra | carried(target)?;
    mount_remount(target, flags, "")
        .with_context(|| format!("restrict the mount at {}", target.display()))
}

fn fresh(target: &Path, kind: &str, flags: MountFlags, options: Option<&str>) -> Result<()> {
    directory(target)?;
    let options = options
        .map(CString::new)
        .transpose()
        .context("encode the mount options")?;
    mount(kind, target, kind, flags, options.as_deref())
        .with_context(|| format!("mount {kind} at {}", target.display()))
}

fn run(root: &Path, op: &MountOp) -> Result<()> {
    match op {
        MountOp::Link { name, target } => {
            let link = inside(root, name);
            if let Some(parent) = link.parent() {
                directory(parent)?;
            }
            symlink(target, &link).with_context(|| format!("link {}", link.display()))
        }
        MountOp::BindReadOnly { source, target } => {
            let target = inside(root, target);
            bind(source, &target)?;
            restrict(&target, MountFlags::RDONLY)
        }
        MountOp::BindReadWrite { source, target } => {
            let target = inside(root, target);
            bind(source, &target)?;
            restrict(&target, MountFlags::empty())
        }
        MountOp::BindDevice(device) => bind(device, &inside(root, device)),
        MountOp::Tmpfs {
            target,
            bytes,
            mode,
        } => fresh(
            &inside(root, target),
            "tmpfs",
            MountFlags::NOSUID | MountFlags::NODEV,
            Some(&format!("size={bytes},mode={mode:o}")),
        ),
        MountOp::SealReadOnly(target) => mount_remount(
            inside(root, target),
            MountFlags::RDONLY | MountFlags::NOSUID | MountFlags::NODEV,
            "",
        )
        .with_context(|| format!("seal {}", target.display())),
        MountOp::Proc => fresh(
            &inside(root, Path::new("/proc")),
            "proc",
            MountFlags::NOSUID | MountFlags::NODEV | MountFlags::NOEXEC,
            None,
        ),
    }
}

fn pivot(root: &Path) -> Result<()> {
    let old = root.join(OLD_ROOT);
    directory(&old)?;
    pivot_root(root, &old).context("pivot into the constructed root")?;
    std::env::set_current_dir("/").context("enter the constructed root")?;
    let old = Path::new("/").join(OLD_ROOT);
    unmount(&old, UnmountFlags::DETACH).context("detach the old root")?;
    std::fs::remove_dir(&old).context("remove the old root mountpoint")?;
    mount_remount(
        "/",
        MountFlags::RDONLY | MountFlags::NOSUID | MountFlags::NODEV,
        "",
    )
    .context("seal the constructed root")
}

pub(super) fn build(layout: &Layout, ops: &[MountOp]) -> Result<()> {
    fresh(
        &layout.root,
        "tmpfs",
        MountFlags::NOSUID | MountFlags::NODEV,
        Some(&format!("size={ROOT_BYTES},mode={PUBLIC:o}")),
    )?;
    for op in ops {
        run(&layout.root, op).with_context(|| format!("apply {op:?}"))?;
    }
    pivot(&layout.root)
}
