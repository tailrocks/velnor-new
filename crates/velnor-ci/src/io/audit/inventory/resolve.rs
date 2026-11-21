// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::VecDeque;
use std::ffi::OsString;
use std::io;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::path::{Component, Path, PathBuf};

use nix::errno::Errno;
use nix::fcntl::{AT_FDCWD, AtFlags, OFlag, openat, readlinkat};
use nix::sys::stat::{FileStat, Mode, SFlag, fstat, fstatat};

use super::handle::{self, Stamp};
use super::walk::{DESCRIPTOR_BUDGET, UNTRACKED_LISTING};

const MAX_LINKS: usize = 40;
pub(super) const RESOLVER_FIXED: usize = 3;

fn failure(errno: Errno) -> io::Error {
    io::Error::from_raw_os_error(errno as i32)
}

#[cfg(any(target_os = "linux", target_os = "android"))]
const TRAVERSE: OFlag = OFlag::O_PATH;
#[cfg(not(any(target_os = "linux", target_os = "android")))]
const TRAVERSE: OFlag = OFlag::O_SEARCH;

fn open_directory(anchor: BorrowedFd<'_>, name: &Path, listing: bool) -> io::Result<OwnedFd> {
    let access = if listing { OFlag::O_RDONLY } else { TRAVERSE };
    let flags = access | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC;
    openat(anchor, name, flags, Mode::empty()).map_err(failure)
}

pub(super) fn open_path(path: &Path, listing: bool) -> io::Result<OwnedFd> {
    let mut components = path.components();
    if components.next() != Some(Component::RootDir) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{} is not an absolute path", path.display()),
        ));
    }
    let below = components.as_path();
    let root = open_directory(
        AT_FDCWD,
        Path::new("/"),
        listing && below.as_os_str().is_empty(),
    )?;
    descend(root, below, listing)
}

pub(super) fn open_below(
    anchor: BorrowedFd<'_>,
    below: &Path,
    listing: bool,
) -> io::Result<OwnedFd> {
    descend(anchor.try_clone_to_owned()?, below, listing)
}

fn descend(mut current: OwnedFd, below: &Path, listing: bool) -> io::Result<OwnedFd> {
    let mut remaining = below.components().count();
    for component in below.components() {
        remaining -= 1;
        let Component::Normal(name) = component else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{} is not a canonical path", below.display()),
            ));
        };
        current = open_directory(current.as_fd(), Path::new(name), listing && remaining == 0)?;
    }
    Ok(current)
}

#[derive(Clone, Copy)]
pub(super) struct Reach<'a> {
    pub(super) bound: usize,
    pub(super) opened: &'a dyn Fn(),
}

impl Default for Reach<'_> {
    fn default() -> Self {
        Self {
            bound: DESCRIPTOR_BUDGET.saturating_sub(RESOLVER_FIXED + UNTRACKED_LISTING),
            opened: &|| {},
        }
    }
}

struct Trail {
    parents: VecDeque<OwnedFd>,
    lineage: Vec<Stamp>,
    bound: usize,
}

impl Trail {
    fn new(bound: usize) -> Self {
        Self {
            parents: VecDeque::new(),
            lineage: Vec::new(),
            bound,
        }
    }

    fn push(&mut self, parent: OwnedFd) -> io::Result<()> {
        self.lineage.push(handle::stamp(&parent)?);
        while self.parents.len() >= self.bound {
            if self.parents.pop_front().is_none() {
                return Ok(());
            }
        }
        self.parents.push_back(parent);
        Ok(())
    }

    fn pop(&mut self) -> Option<(Option<OwnedFd>, Stamp)> {
        let inode = self.lineage.pop()?;
        Some((self.parents.pop_back(), inode))
    }

    fn clear(&mut self) {
        self.parents.clear();
        self.lineage.clear();
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.parents.len()
    }
}

enum Step {
    Root,
    Parent,
    Name(OsString),
}

fn steps(target: &Path, pending: &mut Vec<Step>) {
    for component in target.components().rev() {
        match component {
            Component::Normal(name) => pending.push(Step::Name(name.to_owned())),
            Component::ParentDir => pending.push(Step::Parent),
            Component::RootDir => pending.push(Step::Root),
            Component::CurDir | Component::Prefix(_) => {}
        }
    }
}

pub(super) struct Followed {
    pub(super) path: PathBuf,
    pub(super) stat: FileStat,
}

pub(super) fn resolve(
    directory: BorrowedFd<'_>,
    base: &Path,
    target: &Path,
    reach: Reach<'_>,
) -> io::Result<Followed> {
    let mut current = directory.try_clone_to_owned()?;
    (reach.opened)();
    let mut trail = Trail::new(reach.bound);
    let mut path = base.to_path_buf();
    let mut pending = Vec::new();
    let mut links = 0;
    let mut last = None;
    steps(target, &mut pending);
    while let Some(step) = pending.pop() {
        match step {
            Step::Root => {
                trail.clear();
                current = open_path(Path::new("/"), false)?;
                (reach.opened)();
                path = PathBuf::from("/");
                last = None;
            }
            Step::Parent => {
                current = match trail.pop() {
                    Some((Some(parent), _)) => parent,
                    Some((None, inode)) => {
                        let parent = open_directory(current.as_fd(), Path::new(".."), false)?;
                        if handle::stamp(&parent)? != inode {
                            return Err(failure(Errno::ESTALE));
                        }
                        parent
                    }
                    None => open_directory(current.as_fd(), Path::new(".."), false)?,
                };
                (reach.opened)();
                path.pop();
                last = None;
            }
            Step::Name(name) => {
                let stat = fstatat(&current, name.as_os_str(), AtFlags::AT_SYMLINK_NOFOLLOW)
                    .map_err(failure)?;
                let format = SFlag::from_bits_truncate(stat.st_mode) & SFlag::S_IFMT;
                last = None;
                if format == SFlag::S_IFLNK {
                    links += 1;
                    if links > MAX_LINKS {
                        return Err(failure(Errno::ELOOP));
                    }
                    let next =
                        PathBuf::from(readlinkat(&current, name.as_os_str()).map_err(failure)?);
                    if next.as_os_str().is_empty() {
                        return Err(failure(Errno::ENOENT));
                    }
                    steps(&next, &mut pending);
                } else if format == SFlag::S_IFDIR {
                    let opened = open_directory(current.as_fd(), Path::new(&name), false)?;
                    (reach.opened)();
                    trail.push(std::mem::replace(&mut current, opened))?;
                    path.push(&name);
                } else if pending.is_empty() {
                    path.push(&name);
                    last = Some(stat);
                } else {
                    return Err(failure(Errno::ENOTDIR));
                }
            }
        }
    }
    let stat = match last {
        Some(stat) => stat,
        None => fstat(&current).map_err(failure)?,
    };
    Ok(Followed { path, stat })
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::symlink;

    use super::*;

    fn base(name: &str) -> PathBuf {
        let base = fs::canonicalize(std::env::temp_dir())
            .unwrap_or_else(|e| unreachable!("{e}"))
            .join(format!("velnor-resolve-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap_or_else(|e| unreachable!("{e}"));
        base
    }

    fn resolve_from(directory: &Path, target: &Path, bound: usize) -> io::Result<PathBuf> {
        let held = open_path(directory, false)?;
        let reach = Reach {
            bound,
            ..Reach::default()
        };
        resolve(held.as_fd(), directory, target, reach).map(|followed| followed.path)
    }

    #[test]
    fn a_trail_never_holds_more_parents_than_its_bound() {
        let mut trail = Trail::new(3);
        for _ in 0..10 {
            trail
                .push(open_path(Path::new("/"), false).unwrap_or_else(|e| unreachable!("{e}")))
                .unwrap_or_else(|e| unreachable!("{e}"));
        }
        assert_eq!(trail.len(), 3);
        trail.clear();
        assert_eq!(trail.len(), 0);
    }

    #[test]
    fn a_target_deeper_than_the_trail_bound_resolves_like_canonicalize() {
        let base = base("deep");
        let mut deep = base.join("start");
        for _ in 0..100 {
            deep.push("a");
        }
        fs::create_dir_all(&deep).unwrap_or_else(|e| unreachable!("{e}"));
        symlink("../../..", deep.join("hop")).unwrap_or_else(|e| unreachable!("{e}"));
        let down = "a/".repeat(100);
        let up = "../".repeat(80);
        let targets = [
            format!("{down}{up}a"),
            format!("{down}{up}../../a/hop/a"),
            format!("{down}hop/{up}"),
        ];
        let start = base.join("start");
        for target in &targets {
            let expected = fs::canonicalize(start.join(target)).ok();
            for bound in [0, 1, 7, DESCRIPTOR_BUDGET, usize::MAX] {
                let resolved = resolve_from(&start, Path::new(target), bound).ok();
                assert_eq!(resolved, expected, "bound {bound}, target {target}");
            }
        }
        fs::remove_dir_all(&base).unwrap_or_else(|e| unreachable!("{e}"));
    }

    #[test]
    fn an_ancestor_moved_beyond_the_trail_is_refused_not_followed_to_its_new_parent() {
        let base = base("moved");
        let start = base.join("start");
        let other = base.join("other");
        fs::create_dir_all(start.join("a/b/c/d/e")).unwrap_or_else(|e| unreachable!("{e}"));
        fs::create_dir_all(&other).unwrap_or_else(|e| unreachable!("{e}"));
        fs::write(start.join("marker"), b"").unwrap_or_else(|e| unreachable!("{e}"));
        fs::write(other.join("marker"), b"").unwrap_or_else(|e| unreachable!("{e}"));
        let held = open_path(&start, false).unwrap_or_else(|e| unreachable!("{e}"));
        let opened = std::cell::Cell::new(0);
        let moved = || {
            opened.set(opened.get() + 1);
            if opened.get() == 6 {
                fs::rename(start.join("a"), other.join("a"))
                    .unwrap_or_else(|e| unreachable!("{e}"));
            }
        };
        let reach = Reach {
            bound: 2,
            opened: &moved,
        };
        let resolved = resolve(
            held.as_fd(),
            &start,
            Path::new("a/b/c/d/e/../../../../../marker"),
            reach,
        );
        fs::remove_dir_all(&base).unwrap_or_else(|e| unreachable!("{e}"));
        let errno = resolved.err().and_then(|error| error.raw_os_error());
        assert_eq!(
            errno,
            Some(Errno::ESTALE as i32),
            "the resolver followed a moved ancestor"
        );
    }

    #[test]
    fn an_ancestor_with_the_same_inode_but_a_changed_stamp_is_refused() {
        let base = base("restamped");
        let start = base.join("start");
        fs::create_dir_all(start.join("a/b/c/d/e")).unwrap_or_else(|e| unreachable!("{e}"));
        fs::write(start.join("marker"), b"").unwrap_or_else(|e| unreachable!("{e}"));
        let held = open_path(&start, false).unwrap_or_else(|e| unreachable!("{e}"));
        let opened = std::cell::Cell::new(0);
        let restamped = || {
            opened.set(opened.get() + 1);
            if opened.get() == 6 {
                fs::File::open(start.join("a"))
                    .and_then(|dir| {
                        dir.set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1))
                    })
                    .unwrap_or_else(|e| unreachable!("{e}"));
            }
        };
        let reach = Reach {
            bound: 2,
            opened: &restamped,
        };
        let resolved = resolve(
            held.as_fd(),
            &start,
            Path::new("a/b/c/d/e/../../../../../marker"),
            reach,
        );
        fs::remove_dir_all(&base).unwrap_or_else(|e| unreachable!("{e}"));
        let errno = resolved.err().and_then(|error| error.raw_os_error());
        assert_eq!(
            errno,
            Some(Errno::ESTALE as i32),
            "the resolver followed an ancestor whose stamp changed"
        );
    }
}
