// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use velnor_model::RepoPath;
use velnor_render::{Owner, lf_normalized, owner_of};
use velnor_rust::METADATA_PATH;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Drift {
    Changed(RepoPath),
    Missing(RepoPath),
    Stale(RepoPath),
    Foreign(RepoPath),
    Unmarked(RepoPath),
}

impl Drift {
    #[must_use]
    pub fn path(&self) -> &RepoPath {
        match self {
            Self::Changed(path)
            | Self::Missing(path)
            | Self::Stale(path)
            | Self::Foreign(path)
            | Self::Unmarked(path) => path,
        }
    }
}

impl fmt::Display for Drift {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Changed(path) => write!(f, "{path} differs from the generated content"),
            Self::Missing(path) => write!(f, "{path} is missing"),
            Self::Stale(path) => write!(f, "{path} is no longer generated"),
            Self::Foreign(path) => write!(
                f,
                "{path} was not written by velnor-gen (no marker, or its content digest does not verify); move it away to let velnor-gen own it"
            ),
            Self::Unmarked(path) => write!(
                f,
                "{path} has no generated header and no generator owns it; delete it or generate it"
            ),
        }
    }
}

#[must_use]
pub fn drift(
    expected: &BTreeMap<RepoPath, String>,
    workflows: &BTreeMap<RepoPath, Vec<u8>>,
    engine_configured: bool,
) -> Vec<Drift> {
    let paths: BTreeSet<&RepoPath> = expected.keys().chain(workflows.keys()).collect();
    paths
        .into_iter()
        .filter_map(|path| match (expected.get(path), workflows.get(path)) {
            (Some(want), Some(have)) if *path == METADATA_PATH => {
                (want.as_bytes() != have.as_slice()).then(|| Drift::Changed(path.clone()))
            }
            (None, Some(_)) if *path == METADATA_PATH => Some(Drift::Stale(path.clone())),
            (Some(want), Some(have)) => match owner_of(have) {
                Owner::VelnorGen if want.as_bytes() == lf_normalized(have).as_ref() => None,
                Owner::VelnorGen => Some(Drift::Changed(path.clone())),
                Owner::Forged | Owner::Engine | Owner::Nobody => {
                    Some(Drift::Foreign(path.clone()))
                }
            },
            (Some(_), None) => Some(Drift::Missing(path.clone())),
            (None, Some(have)) => match owner_of(have) {
                Owner::VelnorGen => Some(Drift::Stale(path.clone())),
                Owner::Forged => Some(Drift::Foreign(path.clone())),
                Owner::Engine => None,
                Owner::Nobody => (!engine_configured).then(|| Drift::Unmarked(path.clone())),
            },
            (None, None) => None,
        })
        .collect()
}
