// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::Deserialize;

use crate::error::ReleaseError;

const DEFAULT_PLATFORMS: [&str; 3] = ["linux-x64", "linux-arm64", "macos-arm64"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Binary {
    pub(crate) package: String,
    pub(crate) platforms: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct VelnorConfig {
    pub(crate) roots: Vec<String>,
    pub(crate) after: Vec<String>,
    pub(crate) binaries: Vec<Binary>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawBinary {
    package: String,
    platforms: Option<Vec<String>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Raw {
    roots: Option<Vec<String>>,
    after: Option<Vec<String>>,
    #[serde(default)]
    binaries: Vec<RawBinary>,
}

pub(crate) fn parse(text: Option<&str>) -> Result<VelnorConfig, ReleaseError> {
    let raw = match text {
        Some(text) => toml::from_str::<Raw>(text).map_err(|error| ReleaseError::InvalidToml {
            file: ".velnor/release.toml".to_owned(),
            message: error.to_string(),
        })?,
        None => Raw {
            roots: None,
            after: None,
            binaries: Vec::new(),
        },
    };
    let after = raw.after.unwrap_or_else(|| vec!["Rust CI".to_owned()]);
    if after.is_empty() || after.iter().any(|name| name.trim().is_empty()) {
        return Err(ReleaseError::EmptyAfter);
    }
    let roots = raw.roots.unwrap_or_else(|| vec![".".to_owned()]);
    if roots.is_empty() {
        return Err(ReleaseError::EmptyRoots);
    }
    if let Some(root) = roots.iter().find(|root| {
        root.is_empty() || root.starts_with('/') || root.split('/').any(|part| part == "..")
    }) {
        return Err(ReleaseError::UnsafeRoot { root: root.clone() });
    }
    for binary in &raw.binaries {
        if binary.platforms.as_ref().is_some_and(Vec::is_empty) {
            return Err(ReleaseError::EmptyPlatforms {
                package: binary.package.clone(),
            });
        }
        if let Some(platform) = binary
            .platforms
            .iter()
            .flatten()
            .find(|platform| !DEFAULT_PLATFORMS.contains(&platform.as_str()))
        {
            return Err(ReleaseError::UnknownPlatform {
                package: binary.package.clone(),
                platform: platform.clone(),
            });
        }
    }
    Ok(VelnorConfig {
        roots,
        after,
        binaries: raw
            .binaries
            .into_iter()
            .map(|binary| Binary {
                package: binary.package,
                platforms: binary
                    .platforms
                    .unwrap_or_else(|| DEFAULT_PLATFORMS.iter().map(|p| (*p).to_owned()).collect()),
            })
            .collect(),
    })
}
