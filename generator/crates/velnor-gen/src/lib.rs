// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

pub mod cli;
pub mod io;

use std::collections::BTreeMap;
use std::fmt;

use velnor_model::{RepoPath, RepoView};
use velnor_render::{OWNED_PATHS, render};
use velnor_rust::{DetectError, detect, plan};

pub fn expected_files(view: &RepoView) -> Result<BTreeMap<RepoPath, String>, DetectError> {
    Ok(render(&plan(&detect(view)?)))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Drift {
    Changed(RepoPath),
    Missing(RepoPath),
    Stale(RepoPath),
}

impl Drift {
    #[must_use]
    pub fn path(&self) -> &RepoPath {
        match self {
            Self::Changed(path) | Self::Missing(path) | Self::Stale(path) => path,
        }
    }
}

impl fmt::Display for Drift {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Changed(path) => write!(f, "{path} differs from the generated content"),
            Self::Missing(path) => write!(f, "{path} is missing"),
            Self::Stale(path) => write!(f, "{path} is no longer generated"),
        }
    }
}

#[must_use]
pub fn drift(
    expected: &BTreeMap<RepoPath, String>,
    on_disk: &BTreeMap<RepoPath, Vec<u8>>,
) -> Vec<Drift> {
    OWNED_PATHS
        .iter()
        .filter_map(|path| match (expected.get(path), on_disk.get(path)) {
            (Some(want), Some(have)) if want.as_bytes() == have.as_slice() => None,
            (Some(_), Some(_)) => Some(Drift::Changed(path.clone())),
            (Some(_), None) => Some(Drift::Missing(path.clone())),
            (None, Some(_)) => Some(Drift::Stale(path.clone())),
            (None, None) => None,
        })
        .collect()
}
