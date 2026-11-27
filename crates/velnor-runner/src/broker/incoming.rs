// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsStr;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};

use rustix::fs::renameat;

use super::safe_fs::purge;

pub(super) struct Upload {
    own: OwnedFd,
    pub(super) dir: OwnedFd,
    name: String,
}

impl Upload {
    pub(super) fn new(own: OwnedFd, dir: OwnedFd, name: &str) -> Self {
        Self {
            own,
            dir,
            name: name.to_owned(),
        }
    }

    pub(super) fn discard(&self) {
        let _ = purge(self.own.as_fd(), OsStr::new(&self.name));
    }

    pub(super) fn rename_into(
        &self,
        scope: BorrowedFd<'_>,
        entry: &OsStr,
    ) -> rustix::io::Result<()> {
        renameat(&self.own, self.name.as_str(), scope, entry)
    }
}
