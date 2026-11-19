// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::Deserialize;

use crate::error::ReleaseError;
use crate::plan::ReleaseFiles;
use crate::release_plz::valid_package_name;
use crate::velnor_config::Binary;
use crate::velnor_config::refuse_expressions;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtraBin {
    pub package: String,
    pub bin: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawExtraBin {
    package: Option<String>,
    bin: String,
}

fn invalid(package: &str, entry: &str, reason: &'static str) -> ReleaseError {
    ReleaseError::InvalidBundle {
        package: package.to_owned(),
        entry: entry.to_owned(),
        reason,
    }
}

fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn safe_path(path: &str) -> bool {
    !path.starts_with('-')
        && path
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'/'))
        && path.split('/').all(|part| !matches!(part, "" | "." | ".."))
}

pub(crate) fn parse(
    package: &str,
    primary: &str,
    raw_bins: Vec<RawExtraBin>,
    include: Vec<String>,
) -> Result<(Vec<ExtraBin>, Vec<String>), ReleaseError> {
    let mut names = vec![primary.to_owned()];
    let mut bins = Vec::new();
    for raw in raw_bins {
        let owner = raw.package.unwrap_or_else(|| package.to_owned());
        refuse_expressions("binaries.bins", &[&owner, &raw.bin])?;
        if !valid_package_name(&raw.bin) {
            return Err(ReleaseError::InvalidBinName {
                package: package.to_owned(),
                bin: raw.bin,
            });
        }
        if names.contains(&raw.bin) {
            return Err(invalid(package, &raw.bin, "is listed twice in the asset"));
        }
        names.push(raw.bin.clone());
        bins.push(ExtraBin {
            package: owner,
            bin: raw.bin,
        });
    }
    refuse_expressions(
        "binaries.include",
        &include.iter().map(String::as_str).collect::<Vec<_>>(),
    )?;
    for path in &include {
        if !safe_path(path) {
            return Err(invalid(
                package,
                path,
                "must be a relative path of letters, digits, \".\", \"_\" and \"-\" without \"..\"",
            ));
        }
        if names.iter().any(|name| name == file_name(path)) {
            return Err(invalid(
                package,
                path,
                "has the file name of another asset entry",
            ));
        }
        names.push(file_name(path).to_owned());
    }
    Ok((bins, include))
}

pub(crate) fn check_targets(
    binary: &Binary,
    root_manifest: &str,
    files: &ReleaseFiles,
) -> Result<(), ReleaseError> {
    check_include_files(binary, files)?;
    crate::profile_defined::check_profile(binary, root_manifest, files)?;
    let Some(targets) = &files.bin_targets else {
        return Ok(());
    };
    let primary = (binary.package.as_str(), binary.bin.as_str());
    let wanted: Vec<(&str, &str)> = std::iter::once(primary)
        .chain(
            binary
                .bins
                .iter()
                .map(|extra| (extra.package.as_str(), extra.bin.as_str())),
        )
        .collect();
    let has = |package: &str, bin: &str| {
        targets.contains(&(root_manifest.to_owned(), package.to_owned(), bin.to_owned()))
    };
    for (package, bin) in &wanted {
        if !has(package, bin) {
            return Err(ReleaseError::BinaryTargetMissing {
                package: binary.package.clone(),
                bin: (*bin).to_owned(),
            });
        }
    }
    for (package, _) in &wanted {
        for (_, bin) in wanted.iter().filter(|(owner, _)| owner != package) {
            if has(package, bin) {
                return Err(invalid(
                    &binary.package,
                    bin,
                    "is also a bin of another package built in the same cargo build, which would build both and collide in target/<triple>/release",
                ));
            }
        }
    }
    Ok(())
}

fn check_include_files(binary: &Binary, files: &ReleaseFiles) -> Result<(), ReleaseError> {
    let Some(tracked) = &files.tracked_paths else {
        return Ok(());
    };
    for path in &binary.include {
        let prefix = format!("{path}/");
        if tracked.iter().any(|tracked| tracked.starts_with(&prefix)) {
            return Err(invalid(
                &binary.package,
                path,
                "is a directory, which tar would descend into and break the flat layout of the asset; list its files",
            ));
        }
    }
    Ok(())
}
