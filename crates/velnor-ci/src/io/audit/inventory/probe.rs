// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[cfg(test)]
use std::cell::RefCell;
use std::ffi::OsStr;
use std::io;
#[cfg(not(test))]
use std::marker::PhantomData;
use std::path::Path;
#[cfg(test)]
use std::rc::Rc;

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
type OpenHook = Rc<dyn Fn(&Path) -> io::Result<()>>;
#[cfg(test)]
type ListedHook = Rc<dyn Fn(&OsStr)>;

#[cfg(test)]
#[derive(Default)]
struct Ambient {
    before_open: Option<OpenHook>,
    listed: Option<ListedHook>,
}

#[cfg(test)]
thread_local! {
    static AMBIENT: RefCell<Ambient> = RefCell::new(Ambient::default());
}

#[cfg(test)]
pub(in crate::io::audit) struct Disturbance;

#[cfg(test)]
impl Drop for Disturbance {
    fn drop(&mut self) {
        AMBIENT.with(|ambient| *ambient.borrow_mut() = Ambient::default());
    }
}

#[cfg(test)]
pub(in crate::io::audit) fn disturb(
    before_open: impl Fn(&Path) -> io::Result<()> + 'static,
    listed: impl Fn(&OsStr) + 'static,
) -> Disturbance {
    AMBIENT.with(|ambient| {
        *ambient.borrow_mut() = Ambient {
            before_open: Some(Rc::new(before_open)),
            listed: Some(Rc::new(listed)),
        };
    });
    Disturbance
}

#[cfg(test)]
impl<'a> Probe<'a> {
    pub(super) fn before_open(&self, path: &Path) -> io::Result<()> {
        (self.before_open)(path)?;
        let hook = AMBIENT.with(|ambient| ambient.borrow().before_open.clone());
        hook.map_or(Ok(()), |hook| hook(path))
    }

    pub(super) fn after_open(&self, path: &Path) {
        (self.after_open)(path);
    }

    pub(super) fn listed(&self, name: &OsStr) {
        (self.listed)(name);
        let hook = AMBIENT.with(|ambient| ambient.borrow().listed.clone());
        if let Some(hook) = hook {
            hook(name);
        }
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
