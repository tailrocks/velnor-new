// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/executor.rs

use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io::{self, BufWriter, Read, Write};
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};
use rustix::fs::{
    AtFlags, CWD, Dir, FileType, Mode, OFlags, RawMode, Stat, fstat, openat, readlinkat, statat,
};
use rustix::io::Errno;
use tar::{Builder, EntryType, Header};

use super::descent::{Descent, enter};
use super::guard::{Capped, Watched};
use super::nest::Nesting;
use super::stamp::Stamp;
use crate::broker::cache::MAX_DATA_BYTES;
use crate::broker::safe_fs::directory_flags;
use crate::exec::JobCancel;
use crate::width::widen;

pub(super) const BUFFER: usize = 1 << 20;

#[derive(Debug, thiserror::Error)]
#[error("the job was cancelled")]
pub(super) struct Cancelled;

pub(super) fn checkpoint(cancel: Option<&JobCancel>) -> Result<(), Cancelled> {
    if cancel.is_some_and(JobCancel::is_cancelled) {
        Err(Cancelled)
    } else {
        Ok(())
    }
}

fn header_of(stat: &Stat, kind: EntryType, size: u64) -> Header {
    let mut header = Header::new_gnu();
    header.set_entry_type(kind);
    header.set_mode(widen(stat.st_mode & 0o7777));
    header.set_uid(u64::from(stat.st_uid));
    header.set_gid(u64::from(stat.st_gid));
    header.set_size(size);
    header.set_mtime(u64::try_from(stat.st_mtime).unwrap_or_default());
    header
}

#[derive(PartialEq, Eq)]
struct Identity {
    owner: (u32, u32),
    mode: RawMode,
    mtime: (i64, i128),
    size: Option<i64>,
    node: (i128, i128),
}

fn identity(stat: &Stat) -> Identity {
    let directory = FileType::from_raw_mode(stat.st_mode) == FileType::Directory;
    Identity {
        owner: (stat.st_uid, stat.st_gid),
        mode: stat.st_mode,
        mtime: (stat.st_mtime, i128::from(stat.st_mtime_nsec)),
        size: (!directory).then_some(stat.st_size),
        node: (i128::from(stat.st_dev), i128::from(stat.st_ino)),
    }
}

struct Frame {
    parents: Vec<OsString>,
    member: PathBuf,
    pending: Vec<OsString>,
    seen: Identity,
}

impl Frame {
    fn unchanged(&self, descent: &mut Descent, root: &OwnedFd) -> Result<()> {
        let now = match self.parents.split_last() {
            None => fstat(root)?,
            Some((name, above)) => {
                let parent = descent.at(root.as_fd(), above)?;
                statat(parent, Path::new(name), AtFlags::SYMLINK_NOFOLLOW)
                    .with_context(|| format!("inspect {}", self.member.display()))?
            }
        };
        ensure!(
            identity(&now) == self.seen,
            "{} changed while it was archived",
            self.member.display()
        );
        Ok(())
    }
}

struct Packer<'a, W: Write> {
    builder: Builder<W>,
    cancel: Option<&'a JobCancel>,
    linked: HashMap<(i128, i128), PathBuf>,
}

impl<'a, W: Write> Packer<'a, W> {
    fn new(builder: Builder<W>, cancel: Option<&'a JobCancel>) -> Self {
        Self {
            builder,
            cancel,
            linked: HashMap::new(),
        }
    }

    fn stamp(&mut self, stat: &Stat) -> io::Result<()> {
        match Stamp::of(stat.st_mtime, i128::from(stat.st_mtime_nsec)).pax() {
            Some(text) => self
                .builder
                .append_pax_extensions([("mtime", text.as_bytes())]),
            None => Ok(()),
        }
    }

    fn visit(&mut self, parent: BorrowedFd<'_>, name: &Path, member: &Path) -> Result<()> {
        checkpoint(self.cancel)?;
        let seen = statat(parent, name, AtFlags::SYMLINK_NOFOLLOW)
            .with_context(|| format!("inspect {}", member.display()))?;
        match FileType::from_raw_mode(seen.st_mode) {
            FileType::Directory => {
                let directory = openat(parent, name, directory_flags(), Mode::empty())
                    .with_context(|| format!("open {}", member.display()))?;
                self.tree(&directory, member)
            }
            FileType::RegularFile => self.file(parent, name, member),
            FileType::Symlink => self.link(parent, name, &seen, member),
            other => bail!(
                "{} is a {other:?}, which a cache archive cannot hold",
                member.display()
            ),
        }
    }

    fn tree(&mut self, root: &OwnedFd, member: &Path) -> Result<()> {
        let mut descent = Descent::default();
        let mut frames = vec![self.directory(root, Vec::new(), member.to_path_buf())?];
        while let Some(frame) = frames.last_mut() {
            checkpoint(self.cancel)?;
            let Some(name) = frame.pending.pop() else {
                let done = frames.swap_remove(frames.len() - 1);
                done.unchanged(&mut descent, root)?;
                continue;
            };
            let member = frame.member.join(&name);
            let mut parents = frame.parents.clone();
            let parent = descent
                .at(root.as_fd(), &parents)
                .with_context(|| format!("reopen the parent of {}", member.display()))?;
            let path = Path::new(&name);
            let seen = statat(parent, path, AtFlags::SYMLINK_NOFOLLOW)
                .with_context(|| format!("inspect {}", member.display()))?;
            match FileType::from_raw_mode(seen.st_mode) {
                FileType::Directory => {
                    let child = enter(parent, &name)?;
                    parents.push(name);
                    frames.push(self.directory(&child, parents, member)?);
                }
                FileType::RegularFile => self.file(parent, path, &member)?,
                FileType::Symlink => self.link(parent, path, &seen, &member)?,
                other => bail!(
                    "{} is a {other:?}, which a cache archive cannot hold",
                    member.display()
                ),
            }
        }
        Ok(())
    }

    fn directory(
        &mut self,
        directory: &OwnedFd,
        parents: Vec<OsString>,
        member: PathBuf,
    ) -> Result<Frame> {
        let stat = fstat(directory)?;
        self.stamp(&stat)?;
        let mut header = header_of(&stat, EntryType::Directory, 0);
        self.builder
            .append_data(&mut header, &member, io::empty())?;
        let mut pending = Vec::new();
        for item in Dir::read_from(directory)? {
            let name = OsStr::from_bytes(item?.file_name().to_bytes()).to_owned();
            if name != "." && name != ".." {
                pending.push(name);
            }
        }
        pending.reverse();
        Ok(Frame {
            parents,
            member,
            pending,
            seen: identity(&stat),
        })
    }

    fn file(&mut self, parent: BorrowedFd<'_>, name: &Path, member: &Path) -> Result<()> {
        let fd = openat(
            parent,
            name,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .with_context(|| format!("open {}", member.display()))?;
        let stat = fstat(&fd)?;
        ensure!(
            FileType::from_raw_mode(stat.st_mode) == FileType::RegularFile,
            "{} changed while it was archived",
            member.display()
        );
        if stat.st_nlink > 1 {
            let node = identity(&stat).node;
            if let Some(first) = self.linked.get(&node) {
                let mut header = header_of(&stat, EntryType::Link, 0);
                self.builder.append_link(&mut header, member, first)?;
                return Ok(());
            }
            self.linked.insert(node, member.to_path_buf());
        }
        let size = u64::try_from(stat.st_size).unwrap_or_default();
        ensure!(
            size <= MAX_DATA_BYTES,
            "{} takes the archive over the 10GB limit, not saving cache.",
            member.display()
        );
        self.stamp(&stat)?;
        let mut header = header_of(&stat, EntryType::Regular, size);
        let file = File::from(fd);
        let mut data = Watched {
            inner: (&file).take(size),
            cancel: self.cancel,
        };
        let copied = self.builder.append_data(&mut header, member, &mut data);
        checkpoint(self.cancel)?;
        copied?;
        ensure!(
            data.inner.limit() == 0,
            "{} shrank while it was archived",
            member.display()
        );
        let after = fstat(&file)?;
        ensure!(
            identity(&stat) == identity(&after),
            "{} changed while it was archived",
            member.display()
        );
        Ok(())
    }

    fn link(
        &mut self,
        parent: BorrowedFd<'_>,
        name: &Path,
        stat: &Stat,
        member: &Path,
    ) -> Result<()> {
        let target = readlinkat(parent, name, Vec::new())
            .with_context(|| format!("read the link {}", member.display()))?;
        let after = statat(parent, name, AtFlags::SYMLINK_NOFOLLOW)
            .with_context(|| format!("inspect {}", member.display()))?;
        ensure!(
            identity(&after) == identity(stat)
                && usize::try_from(after.st_size) == Ok(target.to_bytes().len()),
            "{} changed while it was archived",
            member.display()
        );
        self.stamp(stat)?;
        let mut header = header_of(stat, EntryType::Symlink, 0);
        self.builder
            .append_link(&mut header, member, OsStr::from_bytes(target.to_bytes()))?;
        Ok(())
    }
}

fn refuse_nested(roots: &[PathBuf]) -> Result<()> {
    let nesting = Nesting::new(roots);
    for (index, inner) in roots.iter().enumerate() {
        if let Some((outer, below)) = nesting.below(roots, index)? {
            refuse_through_links(&roots[outer], &below, inner)?;
        }
    }
    Ok(())
}

fn refuse_through_links(root: &Path, below: &[OsString], inner: &Path) -> Result<()> {
    let mut at = root.to_path_buf();
    for part in std::iter::once(None).chain(below.iter().map(Some)) {
        at.extend(part);
        match statat(CWD, &at, AtFlags::SYMLINK_NOFOLLOW) {
            Ok(seen) => ensure!(
                FileType::from_raw_mode(seen.st_mode) == FileType::Directory,
                "cache restore never writes through a link: {} is a link or file, so {} below it could not be restored",
                at.display(),
                inner.display()
            ),
            Err(Errno::NOENT) => break,
            Err(error) => {
                return Err(error).with_context(|| format!("archive {}", at.display()));
            }
        }
    }
    Ok(())
}

pub(super) fn pack(roots: &[PathBuf], out: impl Write, cancel: Option<&JobCancel>) -> Result<u64> {
    pack_within(roots, out, cancel, MAX_DATA_BYTES)
}

fn pack_within<W: Write>(
    roots: &[PathBuf],
    out: W,
    cancel: Option<&JobCancel>,
    limit: u64,
) -> Result<u64> {
    let mut packer = Packer::new(
        Builder::new(BufWriter::with_capacity(BUFFER, Capped::new(out, limit))),
        cancel,
    );
    packer.builder.follow_symlinks(false);
    refuse_nested(roots)?;
    let mut archived = 0;
    for (index, root) in roots.iter().enumerate() {
        match statat(CWD, root, AtFlags::SYMLINK_NOFOLLOW) {
            Err(Errno::NOENT) => continue,
            Err(error) => {
                return Err(error).with_context(|| format!("archive {}", root.display()));
            }
            Ok(_) => {}
        }
        packer
            .visit(CWD, root, Path::new(&index.to_string()))
            .with_context(|| format!("archive {}", root.display()))?;
        archived += 1;
    }
    ensure!(
        archived > 0,
        "none of the declared cache paths exists any more"
    );
    let capped = packer
        .builder
        .into_inner()
        .and_then(|writer| {
            writer
                .into_inner()
                .map_err(std::io::IntoInnerError::into_error)
        })
        .context("finish the archive")?;
    let (mut out, written) = capped.into_parts();
    out.flush().context("flush the archive")?;
    Ok(written)
}

pub(super) fn normalize(path: &Path) -> PathBuf {
    let mut normal = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                normal.pop();
            }
            Component::CurDir => {}
            other => normal.push(other),
        }
    }
    normal
}

#[cfg(test)]
mod tests;
