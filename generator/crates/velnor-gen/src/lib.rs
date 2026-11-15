// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

pub mod cli;
pub mod io;
mod release;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use velnor_model::{HelperVersion, RepoFacts, RepoPath, RepoView};
use velnor_release::ReleaseError;
use velnor_render::{Owner, RuleViolation, owner_of, render};
use velnor_rust::{DetectError, check_required_check, check_tools, detect, plan};

pub const COMPATIBLE_HELPER_SERIES: (u64, u64) = (0, 1);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GenerateError {
    Detect(DetectError),
    IncompatibleHelper(HelperVersion),
    Release(ReleaseError),
    ReleaseNeedsHelper,
    ReleaseFileNotUtf8(RepoPath),
    ReleaseRootWithoutCargoToml(String),
    ReleasePackageNotInRoot { root: String, package: String },
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
            Self::Release(error) => error.fmt(f),
            Self::ReleaseNeedsHelper => f.write_str(
                "release.yml runs velnor-ci; pin a release with `velnor-gen pin-helper <version>`",
            ),
            Self::ReleaseFileNotUtf8(path) => write!(f, "{path} is not UTF-8 text"),
            Self::ReleaseRootWithoutCargoToml(root) => write!(
                f,
                ".velnor/release.toml: root {root:?} has no Cargo.toml, so release-plz cannot run there"
            ),
            Self::ReleasePackageNotInRoot { root, package } => write!(
                f,
                "release-plz.toml in {root:?} releases package {package:?}, which is not a member of that Cargo root"
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
    let plan = plan(&facts);
    check_required_check(&facts, &plan)?;
    let mut files = render(&plan).map_err(GenerateError::Rules)?;
    files.extend(release::release_workflow(view, &facts)?);
    Ok(files)
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
                "{path} was not written by velnor-gen; move it away to let velnor-gen own it"
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
            (Some(want), Some(have)) => match owner_of(have) {
                Owner::VelnorGen if want.as_bytes() == have.as_slice() => None,
                Owner::VelnorGen => Some(Drift::Changed(path.clone())),
                Owner::Engine | Owner::Nobody => Some(Drift::Foreign(path.clone())),
            },
            (Some(_), None) => Some(Drift::Missing(path.clone())),
            (None, Some(have)) => match owner_of(have) {
                Owner::VelnorGen => Some(Drift::Stale(path.clone())),
                Owner::Engine => None,
                Owner::Nobody => (!engine_configured).then(|| Drift::Unmarked(path.clone())),
            },
            (None, None) => None,
        })
        .collect()
}
