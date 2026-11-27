// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::ffi::CString;
use std::os::fd::OwnedFd;
use std::path::Path;

use anyhow::{Context, Result};
use rustix::fs::{AtFlags, CWD, Dir, FileType, Mode, OFlags};
use rustix::io::Errno;

use super::remove_entry;

#[derive(PartialEq, Eq)]
enum Sweep {
    Done,
    Replace,
}

fn stem_of(name: &[u8]) -> Option<&[u8]> {
    let end = name.iter().position(|byte| *byte == b'.')?;
    let stem = &name[..end];
    stem.starts_with(b"pack-").then_some(stem)
}

fn remove_orphans(pack: &OwnedFd) -> Result<()> {
    let mut names: Vec<CString> = Vec::new();
    for entry in Dir::read_from(pack).context("read the mirror pack directory")? {
        names.push(
            entry
                .context("read a pack directory entry")?
                .file_name()
                .to_owned(),
        );
    }
    let indexed: BTreeSet<&[u8]> = names
        .iter()
        .filter(|name| name.to_bytes().ends_with(b".idx"))
        .filter_map(|name| stem_of(name.to_bytes()))
        .collect();
    for name in &names {
        let orphaned = stem_of(name.to_bytes()).is_some_and(|stem| !indexed.contains(stem));
        if !orphaned {
            continue;
        }
        match rustix::fs::statat(pack, name, AtFlags::SYMLINK_NOFOLLOW) {
            Ok(stat) if FileType::from_raw_mode(stat.st_mode) == FileType::Directory => {}
            Ok(_) => match rustix::fs::unlinkat(pack, name, AtFlags::empty()) {
                Ok(()) | Err(Errno::NOENT) => {}
                Err(error) => return Err(error).context("remove an orphaned pack file"),
            },
            Err(Errno::NOENT) => {}
            Err(error) => return Err(error).context("inspect a pack directory entry"),
        }
    }
    Ok(())
}

fn sweep(mirror: &Path) -> Result<Sweep> {
    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC;
    let mirror_fd = rustix::fs::openat(CWD, mirror, flags, Mode::empty())
        .with_context(|| format!("open {}", mirror.display()))?;
    let objects = match rustix::fs::openat(
        &mirror_fd,
        "objects",
        flags | OFlags::NOFOLLOW,
        Mode::empty(),
    ) {
        Ok(objects) => objects,
        Err(Errno::NOENT | Errno::LOOP | Errno::NOTDIR) => return Ok(Sweep::Done),
        Err(error) => return Err(error).context("open the mirror objects directory"),
    };
    let pack = match rustix::fs::openat(&objects, "pack", flags | OFlags::NOFOLLOW, Mode::empty()) {
        Ok(pack) => pack,
        Err(Errno::NOENT) => return Ok(Sweep::Done),
        Err(Errno::LOOP | Errno::NOTDIR) => return Ok(Sweep::Replace),
        Err(error) => return Err(error).context("open the mirror pack directory"),
    };
    remove_orphans(&pack)?;
    Ok(Sweep::Done)
}

pub(super) async fn remove_orphan_packs(mirror: &Path) -> Result<()> {
    let target = mirror.to_owned();
    let swept = tokio::task::spawn_blocking(move || sweep(&target))
        .await
        .context("wait for the orphan pack sweep")??;
    if swept == Sweep::Replace {
        let pack = mirror.join("objects/pack");
        remove_entry(&pack).await?;
        tokio::fs::create_dir(&pack)
            .await
            .with_context(|| format!("create {}", pack.display()))?;
    }
    Ok(())
}
