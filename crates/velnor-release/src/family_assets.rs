// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_model::Platform;

use crate::asset_name::expand_asset_name;
use crate::error::ReleaseError;
use crate::error_family::FamilyError;
use crate::plan::BinaryPlan;
use crate::velnor_config::{BinaryFormat, Libc};

const VERSION: &str = "{{ version }}";

fn stem(binary: &BinaryPlan, bin: &str, platform: Platform) -> String {
    let triple = platform.release_triple();
    let target = match binary.libc {
        Libc::Gnu => triple.replace("-musl", "-gnu"),
        Libc::Musl => triple.to_owned(),
    };
    match &binary.asset_name {
        Some(template) => {
            expand_asset_name(template, bin, platform.file_suffix(), &target, VERSION)
        }
        None => match binary.format {
            BinaryFormat::Archive => format!("{bin}-{target}"),
            BinaryFormat::Raw => format!("{bin}-{}", platform.file_suffix()),
        },
    }
}

fn files(binary: &BinaryPlan, platform: Platform) -> Vec<String> {
    match binary.format {
        BinaryFormat::Archive => vec![format!("{}.tar.gz", stem(binary, &binary.bin, platform))],
        BinaryFormat::Raw => std::iter::once(binary.bin.as_str())
            .chain(binary.extra_bins.iter().map(|extra| extra.bin.as_str()))
            .flat_map(|bin| {
                let name = stem(binary, bin, platform);
                [
                    format!("{name}.sha256"),
                    format!("{name}.provenance.json"),
                    name,
                ]
            })
            .collect(),
    }
}

pub(crate) fn check_asset_names(binaries: &[BinaryPlan]) -> Result<(), ReleaseError> {
    let mut owners: BTreeMap<String, &str> = BTreeMap::new();
    for binary in binaries {
        for platform in &binary.platforms {
            for file in files(binary, *platform) {
                if let Some(first) = owners.insert(file.clone(), &binary.package) {
                    return Err(FamilyError::AssetNameCollision {
                        first: first.to_owned(),
                        second: binary.package.clone(),
                        file,
                    }
                    .into());
                }
            }
        }
    }
    Ok(())
}
