// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ops::Range;
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};

use nix::dir::Dir;

use super::handle::{self, Anchor, Climb, Opened, Origin};
use super::probe::Probe;
use super::resolve::{RESOLVER_FIXED, Reach};
use super::walk::{DESCRIPTOR_BUDGET, Pending, UNTRACKED_LISTING};

const RECENT: usize = 4;

struct Retained {
    handle: Weak<Dir>,
    siblings: Range<usize>,
    path: PathBuf,
    depth: usize,
}

pub(super) struct Held<'p, 'a> {
    handles: Vec<Retained>,
    probe: &'p Probe<'a>,
}

impl<'p, 'a> Held<'p, 'a> {
    pub(super) fn new(probe: &'p Probe<'a>) -> Self {
        Self {
            handles: Vec::new(),
            probe,
        }
    }

    pub(super) fn reach(&mut self) -> Reach<'a> {
        self.handles
            .retain(|retained| retained.handle.strong_count() > 0);
        let in_use = self.handles.len() + UNTRACKED_LISTING + RESOLVER_FIXED;
        Reach {
            bound: DESCRIPTOR_BUDGET.saturating_sub(in_use),
            #[cfg(test)]
            opened: self.probe.opened(),
            #[cfg(not(test))]
            marker: std::marker::PhantomData,
        }
    }

    pub(super) fn climb(&self, origin: &Origin) -> Climb<'a> {
        let anchor = match origin {
            Origin::Reopen { parent, .. } => self.nearest(parent),
            Origin::Path | Origin::Child { .. } => None,
        };
        Climb {
            anchor,
            #[cfg(test)]
            stepped: self.probe.reopened(),
            #[cfg(not(test))]
            marker: std::marker::PhantomData,
        }
    }

    fn nearest(&self, parent: &Path) -> Option<Anchor> {
        self.handles.iter().rev().find_map(|retained| {
            if !parent.starts_with(&retained.path) {
                return None;
            }
            Some(Anchor {
                dir: retained.handle.upgrade()?,
                path: retained.path.clone(),
            })
        })
    }

    pub(super) fn track(
        &mut self,
        listing: &Opened,
        directory: &Path,
        pending: &mut [Pending],
        start: usize,
    ) {
        self.handles.push(Retained {
            handle: Rc::downgrade(listing),
            siblings: start..pending.len(),
            path: directory.to_path_buf(),
            depth: directory.components().count(),
        });
        self.handles
            .retain(|retained| retained.handle.strong_count() > 0);
        while self.handles.len() > self.probe.held() {
            let released = self.handles.remove(self.victim());
            let end = released.siblings.end.min(pending.len());
            for entry in pending
                .get_mut(released.siblings.start..end)
                .unwrap_or_default()
            {
                if let Some(reopen) = reopening(entry, &released.handle) {
                    entry.origin = reopen;
                }
            }
        }
    }

    fn victim(&self) -> usize {
        let candidates = self.handles.len().saturating_sub(RECENT);
        let depth = |index: usize| self.handles.get(index).map_or(0, |retained| retained.depth);
        (0..candidates)
            .min_by_key(|&index| {
                let before = index.checked_sub(1).map_or(0, depth);
                depth(index + 1).saturating_sub(before)
            })
            .unwrap_or(0)
    }
}

fn reopening(entry: &Pending, released: &Weak<Dir>) -> Option<Origin> {
    let Origin::Child {
        parent,
        stamp,
        name,
    } = &entry.origin
    else {
        return None;
    };
    if !Weak::ptr_eq(&Rc::downgrade(parent), released) {
        return None;
    }
    Some(Origin::Reopen {
        parent: entry.path.parent()?.to_path_buf(),
        stamp: *stamp,
        child: handle::child_stamp(parent, name),
        name: name.clone(),
    })
}
