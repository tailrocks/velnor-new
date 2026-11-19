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
    pub(crate) attest: bool,
    pub(crate) profile: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct VelnorConfig {
    pub(crate) roots: Vec<String>,
    pub(crate) after: Vec<String>,
    pub(crate) pr_only_checks: Vec<String>,
    pub(crate) binaries: Vec<Binary>,
    pub(crate) family_tag: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawBinary {
    package: String,
    bin: Option<String>,
    platforms: Option<Vec<String>>,
    #[serde(default)]
    attest: bool,
    profile: Option<String>,
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
    family_tag: Option<String>,
}

fn refuse_expressions(key: &str, values: &[&str]) -> Result<(), ReleaseError> {
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
            family_tag: None,
        },
    };
    let after = raw.after.unwrap_or_else(|| vec!["Rust CI".to_owned()]);
    refuse_expressions("pr_only_checks", &text_refs(&raw.pr_only_checks))?;
    for (index, name) in raw.pr_only_checks.iter().enumerate() {
        let problem = if name.trim().is_empty() {
            "is blank; name a required check or remove the entry"
        } else if raw.pr_only_checks[..index].contains(name) {
            "repeats an earlier entry; remove the duplicate"
        } else {
            continue;
        };
        return Err(ReleaseError::InvalidToml {
            file: ".velnor/release.toml".to_owned(),
            message: format!("pr_only_checks[{index}] {problem}"),
        });
    }
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
    refuse_expressions(
        "family_tag",
        &raw.family_tag
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
    )?;
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
        family_tag: raw.family_tag,
    })
}

fn text_refs(values: &[String]) -> Vec<&str> {
    values.iter().map(String::as_str).collect()
}

/// Names Cargo reserves, or that would build into a directory another profile or
/// Cargo itself owns. `dev`, `test` and `bench` land in `target/debug`; the rest are
/// refused by Cargo or collide with its own `target` subdirectories.
const RESERVED_PROFILES: [&str; 11] = [
    "dev",
    "test",
    "bench",
    "debug",
    "doc",
    "build-override",
    "package",
    "deps",
    "examples",
    "build",
    "incremental",
];

fn profile(package: &str, name: Option<String>) -> Result<Option<String>, ReleaseError> {
    let Some(name) = name else {
        return Ok(None);
    };
    refuse_expressions("binaries.profile", &[&name])?;
    if !valid_package_name(&name) || RESERVED_PROFILES.contains(&name.as_str()) {
        return Err(ReleaseError::InvalidProfile(package.to_owned(), name));
    }
    Ok(Some(name))
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
    let profile = profile(&raw.package, raw.profile)?;
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
        attest: raw.attest,
        profile,
    })
}
