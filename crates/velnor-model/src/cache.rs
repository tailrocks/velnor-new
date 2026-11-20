// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::ids::{JobId, RepoPath, RootSlug};

const CARGO_LOCK: RepoPath = RepoPath::from_static("Cargo.lock");

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Host;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CacheRoot {
    slug: RootSlug,
    label: String,
    locks: Vec<RepoPath>,
    target_dir: RepoPath,
    lock: LockIdentity,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LockIdentity {
    #[default]
    File,
    Members,
}

impl CacheRoot {
    #[must_use]
    pub fn new(manifest: &RepoPath, target_dir: RepoPath) -> Self {
        Self {
            slug: RootSlug::of(manifest),
            label: manifest
                .parent()
                .map_or_else(|| ".".to_owned(), |directory| directory.to_string()),
            locks: vec![manifest.sibling(&CARGO_LOCK)],
            target_dir,
            lock: LockIdentity::File,
        }
    }

    #[must_use]
    pub fn group(
        slug: RootSlug,
        label: String,
        locks: Vec<RepoPath>,
        target_dir: RepoPath,
    ) -> Self {
        Self {
            slug,
            label,
            locks,
            target_dir,
            lock: LockIdentity::File,
        }
    }

    #[must_use]
    pub fn with_lock_identity(mut self, lock: LockIdentity) -> Self {
        self.lock = lock;
        self
    }

    #[must_use]
    pub fn lock_identity(&self) -> LockIdentity {
        self.lock
    }

    #[must_use]
    pub fn slug(&self) -> &RootSlug {
        &self.slug
    }

    #[must_use]
    pub fn label(&self) -> &str {
        &self.label
    }

    #[must_use]
    pub fn locks(&self) -> &[RepoPath] {
        &self.locks
    }

    #[must_use]
    pub fn target_dir(&self) -> &RepoPath {
        &self.target_dir
    }

    #[must_use]
    pub fn delta_dir(&self) -> String {
        format!("{}.velnor-delta", self.target_dir)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AnchorSource {
    ThisJob,
    Job(JobId),
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ToolsAnchor {
    Unanchored,
    Restored(AnchorSource),
    Recorded,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CacheKey {
    Tools {
        host: Host,
        anchor: ToolsAnchor,
    },
    Sources {
        host: Host,
        lane: CacheRoot,
    },
    Target {
        job: JobId,
        root: CacheRoot,
        host: Host,
    },
    Delta {
        job: JobId,
        root: CacheRoot,
        host: Host,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CachePath {
    MiseData,
    RustupHome,
    CargoRegistryIndex,
    CargoRegistryCache,
    CargoGitDb,
    Target(RepoPath),
    Delta(CacheRoot),
}

impl CacheKey {
    #[must_use]
    pub const fn family(&self) -> &'static str {
        match self {
            Self::Tools { .. } => "tools",
            Self::Sources { .. } => "sources",
            Self::Target { .. } => "target",
            Self::Delta { .. } => "delta",
        }
    }

    #[must_use]
    pub fn paths(&self) -> Vec<CachePath> {
        match self {
            Self::Tools { .. } => vec![CachePath::MiseData, CachePath::RustupHome],
            Self::Sources { .. } => vec![
                CachePath::CargoRegistryIndex,
                CachePath::CargoRegistryCache,
                CachePath::CargoGitDb,
            ],
            Self::Target { root, .. } => {
                vec![CachePath::Target(root.target_dir().clone())]
            }
            Self::Delta { root, .. } => vec![CachePath::Delta(root.clone())],
        }
    }
}
