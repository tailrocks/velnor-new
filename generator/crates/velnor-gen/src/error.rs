// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt;

use velnor_model::{HelperVersion, RepoPath};
use velnor_release::ReleaseError;
use velnor_render::RuleViolation;
use velnor_rust::DetectError;

use crate::COMPATIBLE_HELPER_SERIES;

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
