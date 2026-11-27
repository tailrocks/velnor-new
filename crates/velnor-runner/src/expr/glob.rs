// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/executor.rs

use super::pattern::{GlobError, Pattern, Seg, align, components_of};
use super::seam::Seam;
use super::walk::Walk;
use std::collections::HashSet;
use std::path::Path;
use std::sync::LazyLock;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

const UNBOUNDED: Duration = Duration::from_hours(365 * 24);

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum DirLinks {
    Ignore,
    Follow,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Hidden {
    Include,
    Skip,
}

#[derive(Clone, Copy)]
pub(crate) struct WalkOptions<'a> {
    pub(crate) links: DirLinks,
    pub(crate) hidden: Hidden,
    pub(crate) stop: Option<&'a AtomicBool>,
}

impl WalkOptions<'static> {
    pub(crate) const HASH_FILES: Self = Self {
        links: DirLinks::Ignore,
        hidden: Hidden::Include,
        stop: None,
    };
}

struct Entry {
    negate: bool,
    files: bool,
    segs: Vec<Seg>,
}

pub(crate) struct PatternSet {
    workspace: Vec<String>,
    entries: Vec<Entry>,
}

impl PatternSet {
    pub(crate) fn new<'a>(
        workspace: &Path,
        home: Option<&Path>,
        lines: impl Iterator<Item = &'a str>,
    ) -> Result<Self, GlobError> {
        let workspace = components_of(workspace)?;
        let patterns = lines
            .map(|line| Pattern::parse(line, &workspace, home))
            .collect::<Result<Vec<_>, _>>()?;
        let mut entries = Vec::new();
        for pattern in patterns {
            if pattern.trailing || !matches!(pattern.segs.last(), Some(Seg::GlobStar)) {
                entries.push(Entry {
                    negate: pattern.negate,
                    files: true,
                    segs: pattern.implicit_descendants()?,
                });
            }
            entries.push(Entry {
                negate: pattern.negate,
                files: !pattern.trailing,
                segs: pattern.segs,
            });
        }
        Ok(Self { workspace, entries })
    }

    pub(super) fn matches_file(&self, path: &[String]) -> Result<bool, GlobError> {
        let mut matched = false;
        for entry in &self.entries {
            if entry.files && align(&entry.segs, path)?.last() == Some(&true) {
                matched = !entry.negate;
            }
        }
        Ok(matched)
    }

    pub(super) fn may_contain(&self, path: &[String]) -> Result<bool, GlobError> {
        for entry in self.entries.iter().filter(|entry| !entry.negate) {
            if align(&entry.segs, path)?.iter().any(|alive| *alive) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub(crate) fn roots(&self) -> Vec<Vec<String>> {
        let candidates: Vec<Vec<String>> = self
            .entries
            .iter()
            .filter(|entry| !entry.negate)
            .map(|entry| {
                entry
                    .segs
                    .iter()
                    .map_while(|seg| seg.literal().map(str::to_owned))
                    .filter(|literal| literal != ".")
                    .collect()
            })
            .collect();
        let known: HashSet<&[String]> = candidates.iter().map(Vec::as_slice).collect();
        let mut included: Vec<Vec<String>> = Vec::new();
        for candidate in &candidates {
            let shadowed = (0..candidate.len()).any(|length| known.contains(&candidate[..length]));
            if !shadowed && !included.contains(candidate) {
                included.push(candidate.clone());
            }
        }
        let mut roots: Vec<Vec<String>> = Vec::new();
        for root in included {
            let clamped = if root.starts_with(&self.workspace) {
                root
            } else if self.workspace.starts_with(&root) {
                self.workspace.clone()
            } else {
                continue;
            };
            if !roots.contains(&clamped) {
                roots.push(clamped);
            }
        }
        roots
    }

    pub(super) fn workspace(&self) -> &[String] {
        &self.workspace
    }

    pub(crate) fn walk<'a>(
        &'a self,
        workspace: &'a Path,
        deadline: Instant,
        seam: &'a Seam,
        options: WalkOptions<'a>,
    ) -> Walk<'a> {
        Walk::new(self, workspace, deadline, seam, options)
    }

    pub(crate) fn walk_following<'a>(
        &'a self,
        workspace: &'a Path,
        options: WalkOptions<'a>,
    ) -> Walk<'a> {
        static FOLLOWING: LazyLock<Seam> = LazyLock::new(Seam::following);
        let deadline = Instant::now() + UNBOUNDED;
        Walk::new(self, workspace, deadline, &FOLLOWING, options)
    }
}
