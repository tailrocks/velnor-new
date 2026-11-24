// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsStr;
use std::io;
#[cfg(not(test))]
use std::marker::PhantomData;
use std::path::Path;

use super::walk::HELD_HANDLES;

#[cfg(test)]
pub(super) struct Probe<'a> {
    pub(super) before_open: &'a dyn Fn(&Path) -> io::Result<()>,
    pub(super) after_open: &'a dyn Fn(&Path),
    pub(super) listed: &'a dyn Fn(&OsStr),
    pub(super) opened: &'a dyn Fn(),
    pub(super) reopened: &'a dyn Fn(&Path, usize),
    pub(super) held: usize,
}

#[cfg(not(test))]
pub(super) struct Probe<'a>(PhantomData<&'a ()>);

impl Default for Probe<'_> {
    #[cfg(test)]
    fn default() -> Self {
        Self {
            before_open: &|_| Ok(()),
            after_open: &|_| {},
            listed: &|_| {},
            opened: &|| {},
            reopened: &|_, _| {},
            held: HELD_HANDLES,
        }
    }

    #[cfg(not(test))]
    fn default() -> Self {
        Self(PhantomData)
    }
}

#[cfg(test)]
impl<'a> Probe<'a> {
    pub(super) fn before_open(&self, path: &Path) -> io::Result<()> {
        (self.before_open)(path)
    }

    pub(super) fn after_open(&self, path: &Path) {
        (self.after_open)(path);
    }

    pub(super) fn listed(&self, name: &OsStr) {
        (self.listed)(name);
    }

    pub(super) fn opened(&self) -> &'a dyn Fn() {
        self.opened
    }

    pub(super) fn reopened(&self) -> &'a dyn Fn(&Path, usize) {
        self.reopened
    }

    pub(super) fn held(&self) -> usize {
        self.held
    }
}

#[cfg(not(test))]
#[expect(
    clippy::unused_self,
    clippy::unnecessary_wraps,
    reason = "production build has no test hooks"
)]
impl Probe<'_> {
    pub(super) fn before_open(&self, _path: &Path) -> io::Result<()> {
        Ok(())
    }

    pub(super) fn after_open(&self, _path: &Path) {}

    pub(super) fn listed(&self, _name: &OsStr) {}

    pub(super) fn held(&self) -> usize {
        HELD_HANDLES
    }
}
