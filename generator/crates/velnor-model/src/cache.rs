// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::ids::{JobId, RepoPath, RootSlug};

const CARGO_LOCK: RepoPath = RepoPath::from_static("Cargo.lock");

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Host;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CacheRoot {
    slug: RootSlug,
    manifest: RepoPath,
    target_dir: RepoPath,
}

impl CacheRoot {
    #[must_use]
    pub fn new(manifest: &RepoPath, target_dir: RepoPath) -> Self {
        Self {
            slug: RootSlug::of(manifest),
            manifest: manifest.clone(),
            target_dir,
        }
    }

    #[must_use]
    pub fn slug(&self) -> &RootSlug {
        &self.slug
    }

    #[must_use]
    pub fn lock_file(&self) -> RepoPath {
        self.manifest.sibling(&CARGO_LOCK)
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
pub enum CacheKey {
    Tools {
        host: Host,
    },
    Sources {
        host: Host,
        root: CacheRoot,
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
