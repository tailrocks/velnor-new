// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::Deserialize;
use velnor_model::Platform;

use crate::asset_name;
use crate::bundle::{ExtraBin, RawExtraBin, parse as bundle};
use crate::check::RawCheck;
use crate::error::ReleaseError;
use crate::release_plz::valid_package_name;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BinaryFormat {
    Archive,
    Raw,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Binary {
    pub(crate) package: String,
    pub(crate) bin: String,
    pub(crate) platforms: Vec<Platform>,
    pub(crate) bins: Vec<ExtraBin>,
    pub(crate) include: Vec<String>,
    pub(crate) format: BinaryFormat,
    pub(crate) asset_name: Option<String>,
    pub(crate) attest: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct VelnorConfig {
    pub(crate) roots: Vec<String>,
    pub(crate) after: Vec<String>,
    pub(crate) pr_only_checks: Vec<String>,
    pub(crate) binaries: Vec<Binary>,
    pub(crate) checks: Vec<RawCheck>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawBinary {
    package: String,
    bin: Option<String>,
    platforms: Option<Vec<String>>,
    asset_name: Option<String>,
    #[serde(default)]
    bins: Vec<RawExtraBin>,
    #[serde(default)]
    include: Vec<String>,
    format: Option<BinaryFormat>,
    #[serde(default)]
    attest: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Raw {
    roots: Option<Vec<String>>,
    after: Option<Vec<String>>,
    #[serde(default)]
    pr_only_checks: Vec<String>,
    #[serde(default)]
    binaries: Vec<RawBinary>,
    #[serde(default)]
    checks: Vec<RawCheck>,
}

pub(crate) fn refuse_expressions(key: &str, values: &[&str]) -> Result<(), ReleaseError> {
    match values.iter().find(|value| value.contains("${{")) {
        Some(value) => Err(ReleaseError::ExpressionInValue {
            key: key.to_owned(),
            value: (*value).to_owned(),
        }),
        None => Ok(()),
    }
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
            pr_only_checks: Vec::new(),
            binaries: Vec::new(),
            checks: Vec::new(),
        },
    };
    let after = raw.after.unwrap_or_else(|| vec!["Rust CI".to_owned()]);
    refuse_expressions("pr_only_checks", &text_refs(&raw.pr_only_checks))?;
    refuse_expressions("after", &text_refs(&after))?;
    if after.is_empty() || after.iter().any(|name| name.trim().is_empty()) {
        return Err(ReleaseError::EmptyAfter);
    }
    let roots = raw.roots.unwrap_or_else(|| vec![".".to_owned()]);
    if roots.is_empty() {
        return Err(ReleaseError::EmptyRoots);
    }
    refuse_expressions("roots", &text_refs(&roots))?;
    if let Some(root) = roots.iter().find(|root| {
        root.as_str() != "."
            && (root.is_empty()
                || root.starts_with(['/', '-'])
                || root.split('/').any(|part| matches!(part, "" | "." | "..")))
    }) {
        return Err(ReleaseError::UnsafeRoot { root: root.clone() });
    }
    let binaries = raw
        .binaries
        .into_iter()
        .map(binary)
        .collect::<Result<Vec<_>, _>>()?;
    for (index, entry) in binaries.iter().enumerate() {
        if binaries[..index]
            .iter()
            .any(|earlier| earlier.package == entry.package)
        {
            return Err(ReleaseError::DuplicateBinary {
                package: entry.package.clone(),
            });
        }
    }
    Ok(VelnorConfig {
        roots,
        after,
        pr_only_checks: raw.pr_only_checks,
        binaries,
        checks: raw.checks,
    })
}

fn text_refs(values: &[String]) -> Vec<&str> {
    values.iter().map(String::as_str).collect()
}

fn binary(raw: RawBinary) -> Result<Binary, ReleaseError> {
    refuse_expressions("binaries.package", &[&raw.package])?;
    let bin = raw.bin.unwrap_or_else(|| raw.package.clone());
    refuse_expressions("binaries.bin", &[&bin])?;
    if !valid_package_name(&bin) {
        return Err(ReleaseError::InvalidBinName {
            package: raw.package,
            bin,
        });
    }
    if let Some(template) = &raw.asset_name
        && !asset_name::valid(template)
    {
        return Err(ReleaseError::InvalidAssetName {
            package: raw.package,
            template: template.clone(),
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
    let (bins, include) = bundle(&raw.package, &bin, raw.bins, raw.include)?;
    let format = raw.format.unwrap_or(BinaryFormat::Archive);
    if format == BinaryFormat::Raw
        && let Some(path) = include.first()
    {
        return Err(ReleaseError::InvalidBundle {
            package: raw.package,
            entry: path.clone(),
            reason: "cannot be included in a raw binary, which publishes only executables",
        });
    }
    Ok(Binary {
        package: raw.package,
        bin,
        platforms,
        bins,
        include,
        format,
        asset_name: raw.asset_name,
        attest: raw.attest,
    })
}
