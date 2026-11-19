// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FamilyError {
    InvalidTag { template: String },
    TagWithPackage { template: String },
    WithoutBinaries,
    BinaryReleasedByReleasePlz { package: String },
    Unsupported { key: &'static str },
    NeedsFamilyTag { key: &'static str },
    BinaryKeyBelongsToFamily { key: &'static str, instead: &'static str },
    InvalidChecksumsFile { name: String },
    ChecksumsNeedArchives { package: String },
    TagPushWithReleasedPackage { package: String },
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
            Self::Unsupported { key } => write!(
                f,
                ".velnor/release.toml: family_tag does not support {key} yet; remove it or release the binaries under their own tags"
            ),
            Self::NeedsFamilyTag { key } => write!(
                f,
                ".velnor/release.toml: {key} belongs to a family release; set family_tag or remove {key}"
            ),
            Self::BinaryKeyBelongsToFamily { key, instead } => write!(
                f,
                ".velnor/release.toml: with family_tag, {key} on a binary is refused because one release holds every binary; set {instead} at the top level instead"
            ),
            Self::InvalidChecksumsFile { name } => write!(
                f,
                ".velnor/release.toml: checksums_file {name:?} must be a plain file name of letters, digits, '.', '_' and '-', not starting with '.' or '-', and not SHA256SUMS or COMMIT"
            ),
            Self::ChecksumsNeedArchives { package } => write!(
                f,
                ".velnor/release.toml: checksums_file digests the binaries inside archives, but binary {package} is raw; use format = \"archive\" or remove checksums_file"
            ),
            Self::TagPushWithReleasedPackage { package } => write!(
                f,
                ".velnor/release.toml: family_trigger = \"tag_push\" releases only the family, but release-plz still releases {package}; give it release = false in release-plz.toml or use the workflow_run trigger"
            ),
        }
    }
}

impl From<FamilyError> for crate::error::ReleaseError {
    fn from(error: FamilyError) -> Self {
        Self::Family(error)
    }
}
