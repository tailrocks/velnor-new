// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::{OsStr, OsString};
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};

use anyhow::{Context, Result};

use crate::broker::safe_fs::enter_directory;

const RETAINED: usize = 32;

pub(super) fn enter(parent: BorrowedFd<'_>, name: &OsStr) -> Result<OwnedFd> {
    enter_directory(parent, name).with_context(|| {
        format!(
            "cache restore never writes through a link: {} is not a directory",
            name.display()
        )
    })
}

pub(super) fn descend(start: BorrowedFd<'_>, names: &[OsString]) -> Result<Option<OwnedFd>> {
    let mut current: Option<OwnedFd> = None;
    for name in names {
        let below = current.as_ref().map_or(start, AsFd::as_fd);
        let next = enter(below, name)?;
        current = Some(next);
    }
    Ok(current)
}

#[derive(Default)]
pub(super) struct Descent {
    retained: Vec<(OsString, OwnedFd)>,
    deep: Option<(Vec<OsString>, OwnedFd)>,
}

impl Descent {
    pub(super) fn clear(&mut self) {
        self.retained.clear();
        self.deep = None;
    }

    pub(super) fn at<'a>(
        &'a mut self,
        base: BorrowedFd<'a>,
        parents: &[OsString],
    ) -> Result<BorrowedFd<'a>> {
        let shared = self
            .retained
            .iter()
            .zip(parents)
            .take_while(|((name, _), parent)| name == *parent)
            .count();
        self.retained.truncate(shared);
        let kept = parents.len().min(RETAINED);
        for name in &parents[shared..kept] {
            let below = self.retained.last().map_or(base, |(_, fd)| fd.as_fd());
            let fd = enter(below, name)?;
            self.retained.push((name.clone(), fd));
        }
        let top = self.retained.last().map_or(base, |(_, fd)| fd.as_fd());
        if parents.len() <= RETAINED {
            self.deep = None;
            return Ok(top);
        }
        match self.deep.take() {
            Some((names, fd)) if names == parents => self.deep = Some((names, fd)),
            Some((names, fd)) if parents.starts_with(&names) => {
                let next =
                    descend(fd.as_fd(), &parents[names.len()..])?.context("open a deep path")?;
                self.deep = Some((parents.to_vec(), next));
            }
            _ => {
                let fd = descend(top, &parents[RETAINED..])?.context("open a deep path")?;
                self.deep = Some((parents.to_vec(), fd));
            }
        }
        Ok(self.deep.as_ref().map_or(top, |(_, fd)| fd.as_fd()))
    }
}
