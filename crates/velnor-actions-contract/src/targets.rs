// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ReleaseTarget {
    LinuxX86_64,

    MacosArm64,

    MacosX86_64,
}

impl ReleaseTarget {
    pub const ALL: [Self; 3] = [Self::LinuxX86_64, Self::MacosArm64, Self::MacosX86_64];

    #[must_use]
    pub const fn triple(self) -> &'static str {
        match self {
            Self::LinuxX86_64 => "x86_64-unknown-linux-gnu",
            Self::MacosArm64 => "aarch64-apple-darwin",
            Self::MacosX86_64 => "x86_64-apple-darwin",
        }
    }

    #[must_use]
    pub fn parse_triple(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|target| target.triple() == value)
    }

    #[must_use]
    pub fn for_runner_label(label: &str) -> Option<Self> {
        match label {
            "ubuntu-22.04" | "ubuntu-24.04" | "ubuntu-26.04" => Some(Self::LinuxX86_64),
            "macos-14" | "macos-15" | "macos-15-arm64" | "macos-26" | "xcode-27" => {
                Some(Self::MacosArm64)
            }
            "macos-15-intel" | "macos-26-intel" => Some(Self::MacosX86_64),
            _ => None,
        }
    }
}

pub const SUPPORTED_TARGETS: [&str; 3] = [
    ReleaseTarget::LinuxX86_64.triple(),
    ReleaseTarget::MacosArm64.triple(),
    ReleaseTarget::MacosX86_64.triple(),
];

pub const RELEASE_MANIFEST_FILENAME: &str = "release-manifest.json";

#[must_use]
pub fn is_supported_target(target: &str) -> bool {
    ReleaseTarget::parse_triple(target).is_some()
}

#[must_use]
pub fn asset_filename(version: &str, target: &str) -> String {
    format!("velnor-actions-{version}-{target}")
}

pub const EXPECTED_REPOSITORY: &str = "tailrocks/velnor-new";

const ASSET_HOST: &str = "github.com";

const ASSET_PREFIX: &str = "tailrocks/velnor-new/releases/download/";

fn checked_release_asset_name<'a>(
    url: &'a str,
    version: &str,
    commit: &str,
    file: &str,
    key: &str,
) -> Result<&'a str, crate::errors::ContractError> {
    let bad = || crate::errors::ContractError::config(file, key, "unexpected_artifact_url");
    crate::manifest_checks::check_semver(version, file, "version")?;
    crate::manifest_checks::check_commit(commit, file, "commit")?;
    if url.bytes().any(|b| {
        b.is_ascii_whitespace() || b.is_ascii_control() || matches!(b, b'$' | b'`' | b'?' | b'#')
    }) {
        return Err(bad());
    }
    let Some(rest) = url.strip_prefix("https://") else {
        return Err(bad());
    };
    let Some((host, path)) = rest.split_once('/') else {
        return Err(bad());
    };
    if host != ASSET_HOST {
        return Err(bad());
    }
    let Some(trailer) = path.strip_prefix(ASSET_PREFIX) else {
        return Err(bad());
    };

    let Some((tag, asset)) = trailer.rsplit_once('/') else {
        return Err(bad());
    };
    if asset.is_empty() || !is_release_tag_for_source(tag, version, commit) {
        return Err(bad());
    }
    Ok(asset)
}

fn is_release_tag_for_source(tag: &str, version: &str, commit: &str) -> bool {
    tag.strip_prefix('v')
        .is_some_and(|tag_version| tag_version == version)
        || tag.strip_prefix("generator-").is_some_and(|tag_commit| {
            crate::ids::is_lower_hex_len(tag_commit, 40) && tag_commit == commit
        })
        || is_seed_tag_for_version(tag, version)
}

pub fn check_release_artifact(
    url: &str,
    version: &str,
    commit: &str,
    target: &str,
    file: &str,
    key: &str,
) -> Result<(), crate::errors::ContractError> {
    let asset = checked_release_asset_name(url, version, commit, file, key)?;
    if asset != asset_filename(version, target) {
        return Err(crate::errors::ContractError::config(
            file,
            key,
            "unexpected_artifact_url",
        ));
    }
    Ok(())
}

pub fn check_release_manifest_artifact(
    url: &str,
    version: &str,
    commit: &str,
    file: &str,
    key: &str,
) -> Result<(), crate::errors::ContractError> {
    let asset = checked_release_asset_name(url, version, commit, file, key)?;
    if asset != RELEASE_MANIFEST_FILENAME {
        return Err(crate::errors::ContractError::config(
            file,
            key,
            "unexpected_artifact_url",
        ));
    }
    Ok(())
}

const SEED_TAG_PREFIX: &str = "seed/velnor-actions-";

#[must_use]
pub fn is_seed_tag_for_version(tag: &str, version: &str) -> bool {
    let Some(rest) = tag.strip_prefix(SEED_TAG_PREFIX) else {
        return false;
    };
    if rest == version {
        return true;
    }
    rest.strip_prefix(version).is_some_and(|suffix| {
        suffix.len() > 1
            && suffix.starts_with('-')
            && suffix[1..].bytes().all(|b| b.is_ascii_digit())
    })
}

pub const CONTRACT_VERSION: u32 = 0;
