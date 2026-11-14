// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

pub mod cli;
pub mod io;

use std::collections::BTreeMap;
use std::fmt;

use velnor_model::{HelperVersion, RepoFacts, RepoPath, RepoView};
use velnor_render::{GENERATED_HEADER, OWNED_PATHS, RuleViolation, render};
use velnor_rust::{DetectError, check_tools, detect, plan};

pub const COMPATIBLE_HELPER_SERIES: (u64, u64) = (0, 1);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GenerateError {
    Detect(DetectError),
    IncompatibleHelper(HelperVersion),
    Rules(Vec<RuleViolation>),
}

impl fmt::Display for GenerateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Detect(error) => error.fmt(f),
            Self::IncompatibleHelper(version) => write!(
                f,
                "velnor-ci {version} does not fit this velnor-gen, which works with velnor-ci {}.{}.x; pin a compatible release with `velnor-gen pin-helper <version>`",
                COMPATIBLE_HELPER_SERIES.0, COMPATIBLE_HELPER_SERIES.1
            ),
            Self::Rules(violations) => {
                f.write_str("the generated workflow breaks the YAML rules:")?;
                for violation in violations {
                    write!(f, "\n  {violation}")?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for GenerateError {}

impl From<DetectError> for GenerateError {
    fn from(error: DetectError) -> Self {
        Self::Detect(error)
    }
}

pub fn expected_files(view: &RepoView) -> Result<BTreeMap<RepoPath, String>, GenerateError> {
    let facts = detect(view)?;
    check_helper(&facts)?;
    check_tools(&facts)?;
    render(&plan(&facts)).map_err(GenerateError::Rules)
}

fn check_helper(facts: &RepoFacts) -> Result<(), GenerateError> {
    facts
        .helper
        .as_ref()
        .map_or(Ok(()), |release| require_compatible(release.version()))
}

pub fn require_compatible(version: &HelperVersion) -> Result<(), GenerateError> {
    if version.series() == Some(COMPATIBLE_HELPER_SERIES) {
        Ok(())
    } else {
        Err(GenerateError::IncompatibleHelper(version.clone()))
    }
}

pub fn check_tag(version: &str, tag: &str) -> Result<String, String> {
    let expected = format!("rust-ci-v{version}");
    if tag == expected {
        Ok(format!("tag {tag} names version {version}"))
    } else {
        Err(format!(
            "tag {tag} must be {expected}, because this program is version {version}"
        ))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Drift {
    Changed(RepoPath),
    Missing(RepoPath),
    Stale(RepoPath),
    Foreign(RepoPath),
}

impl Drift {
    #[must_use]
    pub fn path(&self) -> &RepoPath {
        match self {
            Self::Changed(path) | Self::Missing(path) | Self::Stale(path) | Self::Foreign(path) => {
                path
            }
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
                "{path} was not written by velnor-gen; move it away to let velnor-gen own it"
            ),
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
            (_, Some(have)) if !have.starts_with(GENERATED_HEADER.as_bytes()) => {
                Some(Drift::Foreign(path.clone()))
            }
            (Some(want), Some(have)) if want.as_bytes() == have.as_slice() => None,
            (Some(_), Some(_)) => Some(Drift::Changed(path.clone())),
            (Some(_), None) => Some(Drift::Missing(path.clone())),
            (None, Some(_)) => Some(Drift::Stale(path.clone())),
            (None, None) => None,
        })
        .collect()
}
