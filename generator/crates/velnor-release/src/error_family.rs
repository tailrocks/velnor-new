// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FamilyError {
    InvalidTag { template: String },
    TagWithPackage { template: String },
    WithoutBinaries,
    BinaryReleasedByReleasePlz { package: String },
}

impl fmt::Display for FamilyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidTag { template } => write!(
                f,
                ".velnor/release.toml: family_tag {template:?} must be a valid tag name with one {{{{ version }}}}"
            ),
            Self::TagWithPackage { template } => write!(
                f,
                ".velnor/release.toml: family_tag {template:?} names {{{{ package }}}}, but one tag is shared by every binary"
            ),
            Self::WithoutBinaries => write!(
                f,
                ".velnor/release.toml: family_tag needs at least one [[binaries]] entry"
            ),
            Self::BinaryReleasedByReleasePlz { package } => write!(
                f,
                ".velnor/release.toml: binary {package} is released by release-plz under its own tag; with family_tag set, give it a [[package]] with release = false in release-plz.toml so that the family tag is its only tag"
            ),
        }
    }
}

impl From<FamilyError> for crate::error::ReleaseError {
    fn from(error: FamilyError) -> Self {
        Self::Family(error)
    }
}
