// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::Deserialize;
use velnor_model::Platform;

use crate::error::ReleaseError;
use crate::release_plz::valid_package_name;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Binary {
    pub(crate) package: String,
    pub(crate) bin: String,
    pub(crate) platforms: Vec<Platform>,
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
    bin: Option<String>,
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
    let binaries = raw
        .binaries
        .into_iter()
        .map(binary)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(VelnorConfig {
        roots,
        after,
        binaries,
    })
}

fn binary(raw: RawBinary) -> Result<Binary, ReleaseError> {
    let bin = raw.bin.unwrap_or_else(|| raw.package.clone());
    if !valid_package_name(&bin) {
        return Err(ReleaseError::InvalidBinName {
            package: raw.package,
            bin,
        });
    }
    let platforms = match raw.platforms {
        None => Platform::ALL.to_vec(),
        Some(names) if names.is_empty() => {
            return Err(ReleaseError::EmptyPlatforms {
                package: raw.package,
            });
        }
        Some(names) => {
            let mut platforms = Vec::new();
            for name in names {
                let platform = Platform::ALL
                    .into_iter()
                    .find(|platform| platform.file_suffix() == name)
                    .ok_or_else(|| ReleaseError::UnknownPlatform {
                        package: raw.package.clone(),
                        platform: name,
                    })?;
                if !platforms.contains(&platform) {
                    platforms.push(platform);
                }
            }
            platforms
        }
    };
    Ok(Binary {
        package: raw.package,
        bin,
        platforms,
    })
}
