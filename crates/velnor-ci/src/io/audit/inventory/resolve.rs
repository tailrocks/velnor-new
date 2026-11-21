// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::VecDeque;
use std::ffi::OsString;
use std::io;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::path::{Component, Path, PathBuf};

use nix::errno::Errno;
use nix::fcntl::{AT_FDCWD, AtFlags, OFlag, openat, readlinkat};
use nix::sys::stat::{Mode, SFlag, fstatat};

use super::walk::HELD_HANDLES;

const MAX_LINKS: usize = 40;

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
    let mut remaining = components.as_path().components().count();
    let mut current = open_directory(AT_FDCWD, Path::new("/"), listing && remaining == 0)?;
    for component in components {
        remaining -= 1;
        let Component::Normal(name) = component else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{} is not a canonical path", path.display()),
            ));
        };
        current = open_directory(current.as_fd(), Path::new(name), listing && remaining == 0)?;
    }
    Ok(current)
}

struct Trail {
    parents: VecDeque<OwnedFd>,
    bound: usize,
}

impl Trail {
    fn new(bound: usize) -> Self {
        Self {
            parents: VecDeque::new(),
            bound,
        }
    }

    fn push(&mut self, parent: OwnedFd) {
        self.parents.push_back(parent);
        while self.parents.len() > self.bound {
            self.parents.pop_front();
        }
    }

    fn pop(&mut self) -> Option<OwnedFd> {
        self.parents.pop_back()
    }

    fn clear(&mut self) {
        self.parents.clear();
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

/// Canonical path of `target`, a link target found in `directory` (whose canonical
/// path is `base`), resolved link by link through descriptors so no ancestor named
/// by a path is looked up again.
pub(super) fn resolve(
    directory: BorrowedFd<'_>,
    base: &Path,
    target: &Path,
) -> io::Result<PathBuf> {
    resolve_within(directory, base, target, HELD_HANDLES)
}

fn resolve_within(
    directory: BorrowedFd<'_>,
    base: &Path,
    target: &Path,
    bound: usize,
) -> io::Result<PathBuf> {
    let mut current = directory.try_clone_to_owned()?;
    let mut trail = Trail::new(bound);
    let mut path = base.to_path_buf();
    let mut pending = Vec::new();
    let mut links = 0;
    steps(target, &mut pending);
    while let Some(step) = pending.pop() {
        match step {
            Step::Root => {
                current = open_path(Path::new("/"), false)?;
                trail.clear();
                path = PathBuf::from("/");
            }
            Step::Parent => {
                current = match trail.pop() {
                    Some(parent) => parent,
                    None => open_directory(current.as_fd(), Path::new(".."), false)?,
                };
                path.pop();
            }
            Step::Name(name) => {
                let stat = fstatat(&current, name.as_os_str(), AtFlags::AT_SYMLINK_NOFOLLOW)
                    .map_err(failure)?;
                let format = SFlag::from_bits_truncate(stat.st_mode) & SFlag::S_IFMT;
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
                    trail.push(std::mem::replace(&mut current, opened));
                    path.push(&name);
                } else if pending.is_empty() {
                    path.push(&name);
                } else {
                    return Err(failure(Errno::ENOTDIR));
                }
            }
        }
    }
    Ok(path)
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
        resolve_within(held.as_fd(), directory, target, bound)
    }

    #[test]
    fn a_trail_never_holds_more_parents_than_its_bound() {
        let mut trail = Trail::new(3);
        for _ in 0..10 {
            trail.push(open_path(Path::new("/"), false).unwrap_or_else(|e| unreachable!("{e}")));
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
            for bound in [0, 1, 7, HELD_HANDLES, usize::MAX] {
                let resolved = resolve_from(&start, Path::new(target), bound).ok();
                assert_eq!(resolved, expected, "bound {bound}, target {target}");
            }
        }
        fs::remove_dir_all(&base).unwrap_or_else(|e| unreachable!("{e}"));
    }
}
