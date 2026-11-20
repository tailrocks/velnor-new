// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt;

use crate::error::ReleaseError;

impl ReleaseError {
    pub(crate) fn fmt_binary(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownPlatform { package, platform } => write!(
                f,
                ".velnor/release.toml: binary {package} names unknown platform {platform:?}"
            ),
            Self::InvalidBinName { package, bin } => write!(
                f,
                ".velnor/release.toml: binary {package} names bin {bin:?}, which is not a valid binary name"
            ),
            Self::EmptyPlatforms { package } => write!(
                f,
                ".velnor/release.toml: binary {package} must name at least one platform"
            ),
            Self::DuplicateBinary { package } => write!(
                f,
                ".velnor/release.toml: package {package} has more than one [[binaries]] entry"
            ),
            Self::KeyNeedsFormat {
                package,
                key,
                format,
            } => write!(
                f,
                ".velnor/release.toml: binary {package} sets {key}, which needs format = \"{format}\""
            ),
            Self::MuslNeedsLinux { package } => write!(
                f,
                ".velnor/release.toml: binary {package} sets target = \"musl\", which needs a linux platform"
            ),
            Self::InvalidBundle {
                package,
                entry,
                reason,
            } => write!(
                f,
                ".velnor/release.toml: binary {package} bundle entry {entry:?} {reason}"
            ),
            Self::InvalidAssetName { package, template } => write!(
                f,
                ".velnor/release.toml: binary {package} has asset_name {template:?}, which must use only letters, digits, '.', '_', '-' and the placeholders {{{{ bin }}}}, {{{{ platform }}}}, {{{{ target }}}} and {{{{ version }}}}, must name {{{{ platform }}}} or {{{{ target }}}}, may use {{{{ version }}}} only when the tag name ends in the version, and must expand, with a version of up to 32 bytes and the longest sidecar suffix, to a file name of at most 255 bytes"
            ),
            _ => Ok(()),
        }
    }
}
