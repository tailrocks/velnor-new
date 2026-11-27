// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs::DirBuilder;
use std::io::ErrorKind;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt, chown};
use std::path::Path;

use anyhow::{Context, Result, ensure};

use super::args::SandboxArgs;
use super::layout::{cgroup_of, root_of, stage_dir_of, work_dir_of};
use super::{GEN_ROOT, Layout, RootLink, net, tree};

const LINK_NAMES: [&str; 6] = ["bin", "sbin", "lib", "lib32", "lib64", "libx32"];
const PRIVATE: u32 = 0o700;
const STICKY: u32 = 0o1777;
const WRITABLE_BY_OTHERS: u32 = 0o022;

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

pub(super) fn provisioned(path: &Path) -> Result<()> {
    real_directory(path).with_context(|| {
        format!(
            "{} is missing or unsafe; provision the machine before starting a job",
            path.display()
        )
    })?;
    ensure!(
        std::fs::symlink_metadata(path)?.mode() & WRITABLE_BY_OTHERS == 0,
        "{} is writable by group or others",
        path.display()
    );
    Ok(())
}

pub(super) fn layout(args: &SandboxArgs) -> Result<Layout> {
    for path in &args.store_read_only {
        real_directory(path)?;
    }
    Ok(Layout {
        root: root_of(args.reservation),
        cgroup: cgroup_of(args.reservation),
        netns: net::netns_path(args.reservation),
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
    tree::walk(path, true, |_| true).with_context(|| format!("empty {}", path.display()))?;
    match std::fs::remove_dir(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("remove {}", path.display())),
    }
}

pub(super) fn prepare(layout: &Layout) -> Result<()> {
    let generations = Path::new(GEN_ROOT);
    for directory in [generations.join("work"), generations.join("stage")] {
        provisioned(&directory)?;
    }
    for stale in [&layout.work_dir, &layout.stage_dir, &layout.root] {
        remove(stale)?;
    }
    if let Some(parent) = layout.root.parent() {
        create(parent, PRIVATE, None)?;
    }
    let owner = Some((layout.uid, layout.gid));
    create(&layout.root, PRIVATE, None)?;
    create(&layout.work_dir, PRIVATE, owner)?;
    create(&layout.work_dir.join("home"), PRIVATE, owner)?;
    create(&layout.work_dir.join("tmp"), STICKY, owner)?;
    create(&layout.stage_dir, PRIVATE, owner)
}
