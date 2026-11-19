// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::asset_name::checked;
use crate::bundle::check_targets;
use crate::error::ReleaseError;
use crate::error_family::FamilyError;
use crate::family_assets::check_asset_names;
use crate::family_options::FamilyTrigger;
use crate::plan::{BinaryPlan, ReleaseFiles, ReleasePlan, RootPlan, manifest_of, plan_binaries};
use crate::release_plz::RootConfig;
use crate::tag::{TagPattern, family};
use crate::velnor_config::{Binary, BinaryFormat, VelnorConfig};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FamilyPlan {
    pub tag_pattern: TagPattern,
    pub trigger: FamilyTrigger,
    pub checksums_file: Option<String>,
    pub commit_file: bool,
    pub attest: bool,
    pub required_checks: bool,
}

impl FamilyPlan {
    #[must_use]
    pub fn tag_affixes(&self) -> (String, String) {
        let (before, after) = self
            .tag_pattern
            .as_str()
            .split_once(crate::tag::VERSION)
            .unwrap_or_default();
        (before.to_owned(), after.to_owned())
    }
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
        if let Some(error) = unsupported(binary) {
            return Err(error.into());
        }
        if config.family.checksums_file.is_some() && binary.format == BinaryFormat::Raw {
            return Err(FamilyError::ChecksumsNeedArchives {
                package: binary.package.clone(),
            }
            .into());
        }
        check_targets(binary, &manifest_of(&root), files)?;
        binaries.push(BinaryPlan {
            package: binary.package.clone(),
            bin: binary.bin.clone(),
            root,
            platforms: binary.platforms.clone(),
            extra_bins: binary.bins.clone(),
            include: binary.include.clone(),
            asset_name: checked(binary, &tag_pattern)?,
            format: binary.format,
            libc: binary.libc,
            smoke: binary.smoke,
            commit_file: false,
            checksum_files: None,
            member_digests: None,
            attest: false,
            profile: binary.profile.clone(),
            tag_pattern: tag_pattern.clone(),
        });
    }
    check_asset_names(&binaries)?;
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
    let plan = FamilyPlan {
        tag_pattern,
        trigger: config.family.trigger.unwrap_or(FamilyTrigger::WorkflowRun),
        checksums_file: config.family.checksums_file.clone(),
        commit_file: config.family.commit_file,
        attest: config.family.attest,
        required_checks: config.family.required_checks,
    };
    Ok((plan, binaries))
}

fn unsupported(binary: &Binary) -> Option<FamilyError> {
    let on_the_family = |key, instead| FamilyError::BinaryKeyBelongsToFamily { key, instead };
    if binary.attest {
        Some(on_the_family("attest", "attest"))
    } else if binary.commit_file {
        Some(on_the_family("commit_file", "commit_file"))
    } else if binary.member_digests.is_some() {
        Some(on_the_family("member_digests", "checksums_file"))
    } else if binary.checksum_files.is_some() {
        Some(FamilyError::Unsupported {
            key: "checksum_files",
        })
    } else {
        None
    }
}

pub(crate) fn check_trigger(
    family: Option<&FamilyPlan>,
    roots: &[RootPlan],
) -> Result<(), ReleaseError> {
    if family.is_none_or(|family| family.trigger != FamilyTrigger::TagPush) {
        return Ok(());
    }
    let released = roots.iter().find_map(|root| {
        root.packages
            .iter()
            .map(|(package, _)| package)
            .chain(&root.inherited)
            .next()
    });
    match released {
        Some(package) => Err(FamilyError::TagPushWithReleasedPackage {
            package: package.clone(),
        }
        .into()),
        None => Ok(()),
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
