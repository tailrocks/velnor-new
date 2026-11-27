// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/executor.rs

use super::anchor::{Anchor, READ_DIRECTORY, READ_ENTRY, open_directory_below};
use super::glob::PatternSet;
use super::pattern::GlobError;
use super::seam::Seam;
use rustix::fs::{CWD, Dir, FileType, Mode, fstat, openat};
use rustix::io::Errno;
use std::ffi::{OsStr, OsString};
use std::fs::{self, File};
use std::io;
use std::os::fd::OwnedFd;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Instant;

pub(crate) struct Found {
    pub(crate) path: PathBuf,
    pub(crate) file: File,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Hint {
    Directory,
    File,
    Symlink,
    Other,
    Unknown,
}

impl Hint {
    fn of(kind: FileType) -> Self {
        match kind {
            FileType::Directory => Self::Directory,
            FileType::RegularFile => Self::File,
            FileType::Symlink => Self::Symlink,
            FileType::Unknown => Self::Unknown,
            _ => Self::Other,
        }
    }
}

struct Visit {
    path: PathBuf,
    comps: Vec<String>,
    anchor: Anchor,
    name: OsString,
    hint: Hint,
    matched: bool,
    partial: bool,
    missing_ok: bool,
}

struct Child {
    anchor: Anchor,
    name: OsString,
    hint: Hint,
}

pub(crate) struct Walk<'a> {
    patterns: &'a PatternSet,
    workspace: &'a Path,
    deadline: Instant,
    seam: &'a Seam,
    root: Option<Rc<OwnedFd>>,
    roots: std::vec::IntoIter<Vec<String>>,
    stack: Vec<Visit>,
}

fn skip_missing<T>(result: io::Result<T>, path: &Path) -> Result<Option<T>, GlobError> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(GlobError::io(path, error)),
    }
}

fn kind_of(fd: &OwnedFd, path: &Path) -> Result<FileType, GlobError> {
    fstat(fd)
        .map(|stat| FileType::from_raw_mode(stat.st_mode))
        .map_err(|error| GlobError::io(path, error.into()))
}

impl Iterator for Walk<'_> {
    type Item = Result<Found, GlobError>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if Instant::now() >= self.deadline {
                return Some(Err(GlobError::TimedOut));
            }
            let visit = match self.next_visit()? {
                Ok(visit) => visit,
                Err(error) => return Some(Err(error)),
            };
            match self.visit(visit) {
                Ok(Some(found)) => return Some(Ok(found)),
                Ok(None) => {}
                Err(error) => return Some(Err(error)),
            }
        }
    }
}

impl<'a> Walk<'a> {
    pub(super) fn new(
        patterns: &'a PatternSet,
        workspace: &'a Path,
        deadline: Instant,
        seam: &'a Seam,
    ) -> Self {
        Self {
            patterns,
            workspace,
            deadline,
            seam,
            root: None,
            roots: patterns.roots().into_iter(),
            stack: Vec::new(),
        }
    }

    fn root(&mut self) -> Result<Rc<OwnedFd>, GlobError> {
        if let Some(root) = &self.root {
            return Ok(Rc::clone(root));
        }
        let fd = openat(CWD, self.workspace, READ_DIRECTORY, Mode::empty())
            .map_err(|error| GlobError::io(self.workspace, error.into()))?;
        let root = Rc::new(fd);
        self.root = Some(Rc::clone(&root));
        Ok(root)
    }

    fn next_visit(&mut self) -> Option<Result<Visit, GlobError>> {
        while self.stack.is_empty() {
            let root = self.roots.next()?;
            match self.start(&root) {
                Ok(Some(visit)) => return Some(Ok(visit)),
                Ok(None) => {}
                Err(error) => return Some(Err(error)),
            }
        }
        self.stack.pop().map(Ok)
    }

    fn classify(
        &self,
        comps: Vec<String>,
        path: PathBuf,
        child: Child,
        missing_ok: bool,
    ) -> Result<Option<Visit>, GlobError> {
        let matched = self.patterns.matches_file(&comps)?;
        let partial = self.patterns.may_contain(&comps)?;
        if !matched && !partial {
            return Ok(None);
        }
        Ok(Some(Visit {
            path,
            comps,
            anchor: child.anchor,
            name: child.name,
            hint: child.hint,
            matched,
            partial,
            missing_ok,
        }))
    }

    fn locate(
        &mut self,
        searched: &Path,
        name: Option<&String>,
    ) -> Result<Option<(PathBuf, Child)>, GlobError> {
        let (Some(parent), Some(name)) = (searched.parent(), name) else {
            let child = Child {
                anchor: Anchor::root(self.root()?),
                name: OsString::from("."),
                hint: Hint::Unknown,
            };
            return Ok(Some((self.workspace.to_path_buf(), child)));
        };
        let Some(real) = skip_missing(fs::canonicalize(parent), parent)? else {
            return Ok(None);
        };
        let Ok(inside) = real.strip_prefix(self.workspace) else {
            return Ok(None);
        };
        let top = self.root()?;
        let parent_fd = match open_directory_below(&top, inside.iter()) {
            Ok(fd) => fd,
            Err((Errno::NOENT, _)) => return Ok(None),
            Err((error, walked)) => {
                return Err(GlobError::io(&self.workspace.join(walked), error.into()));
            }
        };
        let child = Child {
            anchor: Anchor::root(parent_fd),
            name: OsString::from(name),
            hint: Hint::Unknown,
        };
        Ok(Some((real.join(name), child)))
    }

    fn start(&mut self, root: &[String]) -> Result<Option<Visit>, GlobError> {
        let relative = &root[self.patterns.workspace().len()..];
        let searched = relative
            .iter()
            .fold(self.workspace.to_path_buf(), |path, name| path.join(name));
        if skip_missing(fs::symlink_metadata(&searched), &searched)?.is_none() {
            return Ok(None);
        }
        let Some((path, child)) = self.locate(&searched, relative.last())? else {
            return Ok(None);
        };
        self.classify(root.to_vec(), path, child, true)
    }

    fn visit(&mut self, visit: Visit) -> Result<Option<Found>, GlobError> {
        match visit.hint {
            Hint::Directory if !visit.partial => return Ok(None),
            Hint::File | Hint::Symlink | Hint::Other if !visit.matched => return Ok(None),
            Hint::Other => return Err(not_regular(&visit.path)),
            Hint::Symlink => return self.follow(&visit),
            Hint::Directory | Hint::File | Hint::Unknown => {}
        }
        let possible_directory = visit.hint == Hint::Unknown && !visit.matched;
        let flags = if visit.hint == Hint::Directory || possible_directory {
            READ_DIRECTORY
        } else {
            READ_ENTRY
        };
        let parent = visit
            .anchor
            .parent()
            .map_err(|(error, _)| GlobError::io(&visit.path, error.into()))?;
        self.seam.before_open(&visit.path);
        let fd = match openat(&*parent, visit.name.as_os_str(), flags, Mode::empty()) {
            Ok(fd) => fd,
            Err(Errno::LOOP) if flags == READ_ENTRY => return self.follow(&visit),
            Err(Errno::NOENT) if visit.missing_ok => return Ok(None),
            Err(Errno::NOTDIR | Errno::LOOP | Errno::NXIO | Errno::OPNOTSUPP)
                if possible_directory =>
            {
                return Ok(None);
            }
            Err(Errno::NXIO | Errno::OPNOTSUPP) if visit.matched => {
                return Err(not_regular(&visit.path));
            }
            Err(error) => return Err(GlobError::io(&visit.path, error.into())),
        };
        match kind_of(&fd, &visit.path)? {
            FileType::Directory => {
                if visit.partial {
                    self.push_children(&visit, fd)?;
                }
                Ok(None)
            }
            FileType::RegularFile if visit.matched => Ok(Some(Found {
                path: visit.path,
                file: File::from(fd),
            })),
            _ if !visit.matched => Ok(None),
            _ => Err(not_regular(&visit.path)),
        }
    }

    fn follow(&mut self, visit: &Visit) -> Result<Option<Found>, GlobError> {
        let real =
            fs::canonicalize(&visit.path).map_err(|error| GlobError::io(&visit.path, error))?;
        let Ok(inside) = real.strip_prefix(self.workspace) else {
            return Ok(None);
        };
        let (Some(name), Some(directory)) = (inside.file_name(), inside.parent()) else {
            return Ok(None);
        };
        let top = self.root()?;
        let parent = open_directory_below(&top, directory.iter())
            .map_err(|(error, walked)| GlobError::io(&self.workspace.join(walked), error.into()))?;
        self.seam.before_open(&real);
        let fd = match openat(&*parent, name, READ_ENTRY, Mode::empty()) {
            Ok(fd) => fd,
            Err(Errno::NXIO | Errno::OPNOTSUPP) => return Err(not_regular(&real)),
            Err(error) => return Err(GlobError::io(&real, error.into())),
        };
        match kind_of(&fd, &real)? {
            FileType::Directory => Ok(None),
            FileType::RegularFile => Ok(Some(Found {
                path: real,
                file: File::from(fd),
            })),
            _ => Err(not_regular(&real)),
        }
    }

    fn push_children(&mut self, parent: &Visit, fd: OwnedFd) -> Result<(), GlobError> {
        let reader =
            Dir::read_from(&fd).map_err(|error| GlobError::io(&parent.path, error.into()))?;
        let mut children = Vec::new();
        for item in reader {
            let item = item.map_err(|error| GlobError::io(&parent.path, error.into()))?;
            let bytes = item.file_name().to_bytes();
            if bytes == b"." || bytes == b".." {
                continue;
            }
            let name = OsStr::from_bytes(bytes).to_owned();
            let hint = match name.to_str() {
                Some(_) => Hint::of(item.file_type()),
                None => Hint::Unknown,
            };
            children.push((name, hint));
        }
        children.sort_unstable_by(|left, right| right.0.cmp(&left.0));
        let anchor = parent.anchor.descend(&parent.name, fd);
        for (name, hint) in children {
            let lossy = name.to_string_lossy().into_owned();
            let name = if name.to_str().is_some() {
                name
            } else {
                OsString::from(&lossy)
            };
            let mut comps = parent.comps.clone();
            comps.push(lossy);
            let path = parent.path.join(&name);
            let child = Child {
                anchor: anchor.clone(),
                name,
                hint,
            };
            if let Some(visit) = self.classify(comps, path, child, false)? {
                self.stack.push(visit);
            }
        }
        Ok(())
    }
}

fn not_regular(path: &Path) -> GlobError {
    GlobError::NotRegularFile(path.display().to_string())
}
