// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs::{DirBuilder, remove_dir_all};
use std::io::ErrorKind;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt, chown};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};

use super::args::{SandboxArgs, stage_dir_of, work_dir_of};
use super::{CGROUP_ROOT, Layout, RootLink, SANDBOX_ROOTS};

const LINK_NAMES: [&str; 6] = ["bin", "sbin", "lib", "lib32", "lib64", "libx32"];
const PRIVATE: u32 = 0o700;
const PUBLIC: u32 = 0o755;
const STICKY: u32 = 0o1777;

fn root_links() -> Vec<RootLink> {
    LINK_NAMES
        .iter()
        .filter_map(|name| {
            let target = std::fs::read_link(Path::new("/").join(name)).ok()?;
            Some(RootLink {
                name: (*name).to_owned(),
                target,
            })
        })
        .collect()
}

fn real_directory(path: &Path) -> Result<()> {
    let metadata =
        std::fs::symlink_metadata(path).with_context(|| format!("inspect {}", path.display()))?;
    ensure!(
        metadata.is_dir() && metadata.uid() == 0,
        "{} is not a root-owned directory",
        path.display()
    );
    Ok(())
}

pub(super) fn layout(args: &SandboxArgs) -> Result<Layout> {
    for path in &args.store_read_only {
        real_directory(path)?;
    }
    Ok(Layout {
        root: Path::new(SANDBOX_ROOTS).join(args.reservation.to_string()),
        cgroup: Path::new(CGROUP_ROOT).join(format!("job-{}", args.reservation)),
        uid: args.uid,
        gid: args.gid,
        home: args.home.clone(),
        work_dir: work_dir_of(args.reservation),
        stage_dir: stage_dir_of(args.reservation),
        store_read_only: args.store_read_only.clone(),
        root_links: root_links(),
    })
}

fn create(path: &Path, mode: u32, owner: Option<(u32, u32)>) -> Result<()> {
    DirBuilder::new()
        .recursive(true)
        .mode(mode)
        .create(path)
        .with_context(|| format!("create {}", path.display()))?;
    if let Some((uid, gid)) = owner {
        chown(path, Some(uid), Some(gid))
            .with_context(|| format!("hand {} to the job user", path.display()))?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
            .with_context(|| format!("set the mode of {}", path.display()))?;
    }
    Ok(())
}

pub(super) fn remove(path: &Path) -> Result<()> {
    match remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("remove {}", path.display())),
    }
}

pub(super) fn prepare(layout: &Layout) -> Result<()> {
    let owner = Some((layout.uid, layout.gid));
    for stale in [&layout.work_dir, &layout.stage_dir, &layout.root] {
        remove(stale)?;
    }
    for parent in [&layout.work_dir, &layout.stage_dir, &layout.root] {
        if let Some(parent) = parent.parent() {
            create(parent, PUBLIC, None)?;
        }
    }
    create(&layout.root, PRIVATE, None)?;
    create(&layout.work_dir, PRIVATE, owner)?;
    create(&layout.work_dir.join("home"), PRIVATE, owner)?;
    create(&layout.work_dir.join("tmp"), STICKY, owner)?;
    create(&layout.stage_dir, PRIVATE, owner)
}

pub(super) fn discard(layout: &Layout) -> Vec<PathBuf> {
    [&layout.work_dir, &layout.stage_dir, &layout.root]
        .into_iter()
        .filter(|path| remove(path).is_err())
        .cloned()
        .collect()
}
