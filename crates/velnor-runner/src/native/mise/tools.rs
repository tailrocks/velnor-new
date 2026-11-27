// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::path::{Component, Path, PathBuf};

use rustix::fs::{Mode, OFlags, fstat, open, openat};
use rustix::io::Errno;

use crate::exec::ExecError;

struct Trust {
    owner: u32,
    anchor: PathBuf,
}

#[cfg(feature = "test-support")]
fn declared_trust() -> Option<Trust> {
    let declared = std::env::var("VELNOR_TEST_STORE_TRUST").ok()?;
    let (owner, anchor) = declared.split_once(':')?;
    Some(Trust {
        owner: owner.parse().ok()?,
        anchor: PathBuf::from(anchor),
    })
}

#[cfg(not(feature = "test-support"))]
fn declared_trust() -> Option<Trust> {
    None
}

fn trust() -> Trust {
    declared_trust().unwrap_or_else(|| Trust {
        owner: 0,
        anchor: PathBuf::from("/"),
    })
}

pub(super) struct Tools {
    descriptor: OwnedFd,
    pub(super) real: PathBuf,
}

fn directory() -> OFlags {
    OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC
}

fn require_trusted(
    descriptor: BorrowedFd<'_>,
    path: &Path,
    trust: &Trust,
) -> Result<(), ExecError> {
    if !path.starts_with(&trust.anchor) {
        return Ok(());
    }
    let stats = fstat(descriptor)
        .map_err(|error| ExecError::io(format!("stat {}", path.display()), error.into()))?;
    if stats.st_uid != trust.owner {
        return Err(ExecError::Invalid(format!(
            "mise policy: {} is owned by uid {}; every directory above the tools store must be owned by uid {}",
            path.display(),
            stats.st_uid,
            trust.owner
        )));
    }
    let mode = Mode::from_raw_mode(stats.st_mode);
    if mode.intersects(Mode::WGRP | Mode::WOTH) {
        return Err(ExecError::Invalid(format!(
            "mise policy: {} is writable by its group or others (mode {:o}); no directory above the tools store may be",
            path.display(),
            mode.bits() & 0o7777
        )));
    }
    Ok(())
}

pub(super) fn resolve(tools: &Path) -> PathBuf {
    std::fs::canonicalize(tools).unwrap_or_else(|_| tools.to_path_buf())
}

pub(super) fn open_tools(tools: &Path) -> Result<Option<Tools>, ExecError> {
    let real = match std::fs::canonicalize(tools) {
        Ok(real) => real,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(ExecError::io(format!("resolve {}", tools.display()), error));
        }
    };
    if real != tools {
        return Err(ExecError::Invalid(format!(
            "mise policy: {} is reached through a link ({}); the tools store path must not contain links",
            tools.display(),
            real.display()
        )));
    }
    let trust = trust();
    let mut walked = PathBuf::new();
    let mut current: Option<OwnedFd> = None;
    for component in real.components() {
        walked.push(component);
        let opened = match (component, current.as_ref()) {
            (Component::RootDir, _) => open("/", directory(), Mode::empty()),
            (Component::Normal(name), Some(parent)) => {
                openat(parent, name, directory(), Mode::empty())
            }
            _ => continue,
        };
        let descriptor = match opened {
            Ok(descriptor) => descriptor,
            Err(Errno::NOENT) => return Ok(None),
            Err(error) => {
                return Err(ExecError::io(
                    format!("open {}", walked.display()),
                    error.into(),
                ));
            }
        };
        require_trusted(descriptor.as_fd(), &walked, &trust)?;
        current = Some(descriptor);
    }
    Ok(current.map(|descriptor| Tools { descriptor, real }))
}

impl Tools {
    pub(super) fn open_below(&self, relative: &Path, last: OFlags) -> Result<OwnedFd, Errno> {
        let mut names = relative.iter().peekable();
        let mut current: Option<OwnedFd> = None;
        while let Some(name) = names.next() {
            let flags = if names.peek().is_some() {
                directory()
            } else {
                last | OFlags::NOFOLLOW | OFlags::CLOEXEC
            };
            let parent = current
                .as_ref()
                .map_or(self.descriptor.as_fd(), AsFd::as_fd);
            current = Some(openat(parent, name, flags, Mode::empty())?);
        }
        current.ok_or(Errno::INVAL)
    }
}
