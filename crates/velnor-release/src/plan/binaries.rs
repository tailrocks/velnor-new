// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::bundle::check_targets;
use crate::error::ReleaseError;
use crate::error_family::FamilyError;
use crate::members::{inherited_tag, owner};
use crate::release_plz::RootConfig;
use crate::velnor_config::VelnorConfig;

use super::{BinaryPlan, ReleaseFiles, manifest_of};

pub(crate) fn plan_binaries(
    config: &VelnorConfig,
    configs: &[RootConfig],
    files: &ReleaseFiles,
) -> Result<Vec<BinaryPlan>, ReleaseError> {
    let mut binaries = Vec::new();
    for binary in &config.binaries {
        if !binary.optional_platforms.is_empty() {
            return Err(FamilyError::NeedsFamilyTag {
                key: "optional_platforms",
            }
            .into());
        }
        let (root, tag_pattern) = configs
            .iter()
            .find_map(|candidate| {
                let listed = candidate
                    .packages
                    .iter()
                    .find(|package| package.name == binary.package)
                    .map(|package| package.tag_pattern.clone());
                let inherited = || {
                    let (_, workspace) = owner(&candidate.root, files.rust_metadata.as_ref()?)?;
                    inherited_tag(candidate, workspace, &binary.package)
                };
                listed
                    .or_else(inherited)
                    .map(|pattern| (candidate.root.clone(), pattern))
            })
            .ok_or_else(|| ReleaseError::BinaryNotReleased {
                package: binary.package.clone(),
            })?;
        check_targets(binary, &manifest_of(&root), files)?;
        binaries.push(BinaryPlan {
            package: binary.package.clone(),
            bin: binary.bin.clone(),
            root,
            platforms: binary.platforms.clone(),
            optional_platforms: Vec::new(),
            extra_bins: binary.bins.clone(),
            include: binary.include.clone(),
            format: binary.format,
            libc: binary.libc,
            attest: binary.attest,
            smoke: binary.smoke,
            commit_file: binary.commit_file,
            checksum_files: binary.checksum_files,
            member_digests: binary.member_digests,
            profile: binary.profile.clone(),
            asset_name: crate::asset_name::checked(binary, &tag_pattern)?,
            tag_pattern,
        });
    }
    Ok(binaries)
}
