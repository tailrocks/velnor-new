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
            Self::SmokeNeedsRaw { package } => write!(
                f,
                ".velnor/release.toml: binary {package} sets smoke, which needs format = \"raw\", because an archive holds no executable to run"
            ),
            Self::Sha256FilesNeedsArchive { package } => write!(
                f,
                ".velnor/release.toml: binary {package} sets sha256_files, which needs the archive format, because a raw binary always publishes its own .sha256 file"
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
                ".velnor/release.toml: binary {package} has asset_name {template:?}, which must use only letters, digits, '.', '_', '-' and the placeholders {{{{ bin }}}}, {{{{ platform }}}}, {{{{ target }}}} and {{{{ version }}}}, must name {{{{ platform }}}} or {{{{ target }}}}, and may use {{{{ version }}}} only when the tag name ends in the version"
            ),
            _ => Ok(()),
        }
    }
}
