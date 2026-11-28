// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/executor.rs

use std::ffi::{OsStr, OsString};
use std::io::{BufReader, ErrorKind, Read};
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result};
use rustix::fs::{CWD, Mode, fchmod, openat};
use rustix::io::Errno;
use tar::{Archive, Entry, EntryType};

use super::archive::{BUFFER, checkpoint};
use super::descent::{Descent, descend};
use super::nest::{Link, Nesting, Parent};
use super::place::{Placed, link_file, mode_of, place, remove_existing};
use super::stamp::Stamp;
use crate::broker::safe_fs::{directory_flags, enter_directory, open_regular};
use crate::exec::JobCancel;
use crate::native::Umask;

#[derive(Debug, thiserror::Error)]
#[error("the cache entry vanished before it was opened")]
pub(super) struct Vanished;

fn escapes(member: &Path) -> anyhow::Error {
    anyhow::anyhow!(
        "cache archive member '{}' escapes the declared paths",
        member.display()
    )
}

fn split(member: &Path, roots: &[PathBuf]) -> Result<(usize, Vec<OsString>)> {
    let mut parts = member.components();
    let index = match parts.next() {
        Some(Component::Normal(name)) => name
            .to_str()
            .and_then(|text| text.parse::<usize>().ok())
            .filter(|index| *index < roots.len()),
        _ => None,
    }
    .ok_or_else(|| escapes(member))?;
    let rest = parts
        .map(|part| match part {
            Component::Normal(name) => Ok(name.to_owned()),
            _ => Err(escapes(member)),
        })
        .collect::<Result<_>>()?;
    Ok((index, rest))
}

enum Root {
    Fresh,
    Open(OwnedFd),
    Closed,
}

struct Restore {
    paths: Vec<PathBuf>,
    nesting: Nesting,
    parents: Vec<Parent>,
    roots: Vec<Root>,
    current: Option<usize>,
    descent: Descent,
    pending: Vec<Pending>,
    umask: Umask,
}

struct Pending {
    index: usize,
    path: Vec<OsString>,
    bits: u32,
    stamp: Option<Stamp>,
}

fn cycle(path: &Path) -> anyhow::Error {
    anyhow::anyhow!(
        "the declared path {} and another declared path each resolve below the other",
        path.display()
    )
}

fn closed(path: &Path) -> anyhow::Error {
    anyhow::anyhow!(
        "cache restore never writes through a link: the declared path {} was restored as a link or file",
        path.display()
    )
}

fn open_root(parent: BorrowedFd<'_>, name: &OsStr) -> Result<OwnedFd> {
    match openat(parent, name, directory_flags(), Mode::empty()) {
        Ok(fd) => return Ok(fd),
        Err(Errno::LOOP | Errno::NOTDIR) => remove_existing(parent, name)?,
        Err(_) => {}
    }
    enter_directory(parent, name).with_context(|| format!("open {}", name.display()))
}

impl Restore {
    fn new(declared: &[PathBuf], umask: Umask) -> Self {
        Self {
            paths: declared.to_vec(),
            nesting: Nesting::new(declared),
            parents: declared.iter().map(|_| Parent::Unresolved).collect(),
            roots: declared.iter().map(|_| Root::Fresh).collect(),
            current: None,
            descent: Descent::default(),
            pending: Vec::new(),
            umask,
        }
    }

    fn name(&self, index: usize) -> Result<OsString> {
        Ok(self.paths[index]
            .file_name()
            .context("name a declared path")?
            .to_owned())
    }

    fn parent(&mut self, index: usize) -> Result<BorrowedFd<'_>> {
        if !matches!(self.parents[index], Parent::Ready(_)) {
            let fd = self.open_parent(index)?;
            self.parents[index] = Parent::Ready(fd);
        }
        match &self.parents[index] {
            Parent::Ready(fd) => Ok(fd.as_fd()),
            Parent::Unresolved | Parent::Resolving => Err(closed(&self.paths[index])),
        }
    }

    fn open_parent(&mut self, index: usize) -> Result<OwnedFd> {
        if matches!(self.parents[index], Parent::Resolving) {
            return Err(cycle(&self.paths[index]));
        }
        self.parents[index] = Parent::Resolving;
        match self.nesting.resolve(&self.paths, index)? {
            Link::Free(fd) => Ok(fd),
            Link::Below { outer, below } => {
                if matches!(self.roots[outer], Root::Fresh) {
                    self.open(outer)?;
                }
                let Root::Open(base) = &self.roots[outer] else {
                    return Err(closed(&self.paths[outer]));
                };
                match descend(base.as_fd(), &below)? {
                    Some(fd) => Ok(fd),
                    None => Ok(base.try_clone()?),
                }
            }
        }
    }

    fn open(&mut self, index: usize) -> Result<()> {
        match self.roots[index] {
            Root::Open(_) => return Ok(()),
            Root::Closed => return Err(closed(&self.paths[index])),
            Root::Fresh => {}
        }
        let name = self.name(index)?;
        let root = open_root(self.parent(index)?, &name)?;
        self.roots[index] = Root::Open(root);
        self.nesting.note(&self.paths, index);
        Ok(())
    }

    fn root_member<R: Read>(
        &mut self,
        index: usize,
        entry: &mut Entry<'_, R>,
        stamp: Option<Stamp>,
        cancel: Option<&JobCancel>,
    ) -> Result<()> {
        if entry.header().entry_type() == EntryType::Directory {
            self.open(index)?;
            let bits = self.umask.restored_mode(entry.header());
            self.pending.push(Pending {
                index,
                path: Vec::new(),
                bits,
                stamp,
            });
            return Ok(());
        }
        if matches!(self.roots[index], Root::Closed) {
            return Err(closed(&self.paths[index]));
        }
        let name = self.name(index)?;
        let umask = self.umask;
        place(self.parent(index)?, &name, entry, stamp, umask, cancel)?;
        self.roots[index] = Root::Closed;
        self.nesting.note(&self.paths, index);
        Ok(())
    }

    fn directory(&mut self, index: usize, parents: &[OsString]) -> Result<BorrowedFd<'_>> {
        self.open(index)?;
        if self.current != Some(index) {
            self.descent.clear();
            self.current = Some(index);
        }
        let Self {
            paths,
            roots,
            descent,
            ..
        } = self;
        let Root::Open(base) = &roots[index] else {
            return Err(closed(&paths[index]));
        };
        descent.at(base.as_fd(), parents)
    }

    fn slot(&mut self, index: usize, rest: &[OsString]) -> Result<(OwnedFd, OsString)> {
        match rest.split_last() {
            None => Ok((self.parent(index)?.try_clone_to_owned()?, self.name(index)?)),
            Some((name, parents)) => Ok((
                self.directory(index, parents)?.try_clone_to_owned()?,
                name.clone(),
            )),
        }
    }

    fn hard_link(
        &mut self,
        index: usize,
        rest: &[OsString],
        target: &Path,
        roots: &[PathBuf],
    ) -> Result<()> {
        let (from_index, from_rest) = split(target, roots)?;
        let (source, from) = self.slot(from_index, &from_rest)?;
        if rest.is_empty() && matches!(self.roots[index], Root::Open(_) | Root::Closed) {
            return Err(closed(&self.paths[index]));
        }
        let (parent, name) = self.slot(index, rest)?;
        link_file(source.as_fd(), &from, parent.as_fd(), &name)?;
        if rest.is_empty() {
            self.roots[index] = Root::Closed;
            self.nesting.note(&self.paths, index);
        }
        Ok(())
    }

    fn finish(&mut self, cancel: Option<&JobCancel>) -> Result<()> {
        while let Some(done) = self.pending.pop() {
            checkpoint(cancel)?;
            let Root::Open(base) = &self.roots[done.index] else {
                return Err(closed(&self.paths[done.index]));
            };
            let below = descend(base.as_fd(), &done.path)?;
            let directory = below.as_ref().map_or(base.as_fd(), AsFd::as_fd);
            fchmod(directory, mode_of(done.bits))
                .with_context(|| format!("set the mode of {}", self.paths[done.index].display()))?;
            if let Some(stamp) = done.stamp {
                stamp.apply(directory)?;
            }
        }
        Ok(())
    }
}

pub(super) fn unpack(
    archive: &Path,
    roots: &[PathBuf],
    umask: Umask,
    cancel: Option<&JobCancel>,
) -> Result<u64> {
    let file = match open_regular(CWD, archive) {
        Ok(file) => file,
        Err(error) if error.kind() == ErrorKind::NotFound => return Err(Vanished.into()),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("open {} as a regular file", archive.display()));
        }
    };
    let mut tar = Archive::new(BufReader::with_capacity(BUFFER, file));
    let mut restore = Restore::new(roots, umask);
    let mut members = 0_u64;
    for entry in tar.entries().context("read the cache archive")? {
        checkpoint(cancel)?;
        let mut entry = entry.context("read a cache archive member")?;
        members += 1;
        let stamp = Stamp::of_entry(&mut entry);
        let member = entry
            .path()
            .context("read a cache member path")?
            .into_owned();
        let (index, rest) = split(&member, roots)?;
        if entry.header().entry_type() == EntryType::Link {
            let target = entry
                .link_name()
                .context("read a hard link target")?
                .context("a hard link member has no target")?
                .into_owned();
            restore
                .hard_link(index, &rest, &target, roots)
                .with_context(|| format!("restore {}", member.display()))?;
            continue;
        }
        let Some((name, parents)) = rest.split_last() else {
            restore
                .root_member(index, &mut entry, stamp, cancel)
                .with_context(|| format!("restore {}", roots[index].display()))?;
            continue;
        };
        let parent = restore
            .directory(index, parents)
            .with_context(|| format!("restore {}", member.display()))?;
        let placed = place(parent, name, &mut entry, stamp, umask, cancel)
            .with_context(|| format!("restore {}", member.display()))?;
        if let Placed::Directory { bits } = placed {
            restore.pending.push(Pending {
                index,
                path: rest,
                bits,
                stamp,
            });
        }
    }
    restore.finish(cancel)?;
    Ok(members)
}

#[cfg(test)]
mod tests;
