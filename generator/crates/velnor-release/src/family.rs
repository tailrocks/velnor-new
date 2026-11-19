// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::bundle::check_targets;
use crate::error::ReleaseError;
use crate::error_family::FamilyError;
use crate::plan::{BinaryPlan, ReleaseFiles, ReleasePlan, manifest_of, plan_binaries};
use crate::release_plz::RootConfig;
use crate::tag::{TagPattern, family};
use crate::velnor_config::{Binary, VelnorConfig};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FamilyPlan {
    pub tag_pattern: TagPattern,
}

pub(crate) fn plan_family(
    config: &VelnorConfig,
    template: &str,
    configs: &[RootConfig],
    files: &ReleaseFiles,
) -> Result<(FamilyPlan, Vec<BinaryPlan>), ReleaseError> {
    let tag_pattern = family(template)?;
    if config.binaries.is_empty() {
        return Err(FamilyError::WithoutBinaries.into());
    }
    let mut binaries = Vec::new();
    for binary in &config.binaries {
        let root = configs
            .iter()
            .find(|candidate| candidate.excludes(&binary.package))
            .map(|candidate| candidate.root.clone())
            .ok_or_else(|| FamilyError::BinaryReleasedByReleasePlz {
                package: binary.package.clone(),
            })?;
        if let Some(key) = unsupported(binary) {
            return Err(FamilyError::Unsupported { key }.into());
        }
        check_targets(binary, &manifest_of(&root), files)?;
        binaries.push(BinaryPlan {
            package: binary.package.clone(),
            bin: binary.bin.clone(),
            root,
            platforms: binary.platforms.clone(),
            extra_bins: Vec::new(),
            include: Vec::new(),
            asset_name: None,
            format: binary.format,
            libc: binary.libc,
            smoke: false,
            commit_file: false,
            checksum_files: None,
            member_digests: None,
            attest: binary.attest,
            profile: binary.profile.clone(),
            tag_pattern: tag_pattern.clone(),
        });
    }
    for configured in configs {
        let released = configured
            .packages
            .iter()
            .map(|package| &package.tag_pattern)
            .chain(&configured.workspace_tags);
        for pattern in released {
            if pattern.may_collide(&tag_pattern) {
                return Err(ReleaseError::TagsMayCollide {
                    first: tag_pattern.as_str().to_owned(),
                    second: pattern.as_str().to_owned(),
                });
            }
        }
    }
    Ok((FamilyPlan { tag_pattern }, binaries))
}

fn unsupported(binary: &Binary) -> Option<&'static str> {
    if !binary.bins.is_empty() {
        Some("bins")
    } else if !binary.include.is_empty() {
        Some("include")
    } else if binary.asset_name.is_some() {
        Some("asset_name")
    } else if binary.attest {
        Some("attest")
    } else if binary.smoke {
        Some("smoke")
    } else if binary.commit_file {
        Some("commit_file")
    } else if binary.checksum_files.is_some() {
        Some("checksum_files")
    } else if binary.member_digests.is_some() {
        Some("member_digests")
    } else {
        None
    }
}

pub(crate) fn plan_binaries_and_family(
    config: &VelnorConfig,
    configs: &[RootConfig],
    files: &ReleaseFiles,
) -> Result<(Option<FamilyPlan>, Vec<BinaryPlan>), ReleaseError> {
    match config.family_tag.as_deref() {
        Some(template) => {
            let (plan, binaries) = plan_family(config, template, configs, files)?;
            Ok((Some(plan), binaries))
        }
        None => Ok((None, plan_binaries(config, configs, files)?)),
    }
}

impl ReleasePlan {
    #[must_use]
    pub fn tagged_binaries(&self) -> &[BinaryPlan] {
        if self.family.is_some() {
            &[]
        } else {
            &self.binaries
        }
    }
}
