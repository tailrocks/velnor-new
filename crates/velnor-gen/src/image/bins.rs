// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{ImageConfig, RepoFacts, RustMember, RustRoot, TargetKind};

use crate::ImageError;

pub(crate) fn root_directory(root: &RustRoot) -> &str {
    root.manifest
        .as_str()
        .rsplit_once('/')
        .map_or(".", |(directory, _)| directory)
}

pub(crate) fn root_of<'a>(
    facts: &'a RepoFacts,
    image: &ImageConfig,
) -> Result<&'a RustRoot, ImageError> {
    match &image.root {
        Some(root) => facts
            .rust_roots
            .iter()
            .find(|candidate| root_directory(candidate) == root)
            .ok_or_else(|| ImageError::UnknownRoot {
                image: image.name.clone(),
                root: root.clone(),
            }),
        None => match facts.rust_roots.as_slice() {
            [only] => Ok(only),
            _ => Err(ImageError::AmbiguousRoot {
                image: image.name.clone(),
            }),
        },
    }
}

pub(crate) fn packages_of<'a>(
    image: &ImageConfig,
    root: &'a RustRoot,
) -> Result<Vec<&'a str>, ImageError> {
    let mut owners: Vec<&RustMember> = Vec::new();
    for bin in &image.bins {
        let found: Vec<&RustMember> = root
            .members
            .iter()
            .filter(|member| {
                image
                    .package
                    .as_deref()
                    .is_none_or(|package| member.name.as_str() == package)
                    && member.targets.iter().any(|target| {
                        target.kinds.contains(&TargetKind::Bin) && target.name.as_str() == bin
                    })
            })
            .collect();
        let member = match found.as_slice() {
            [] if image.package.is_some() => {
                return Err(ImageError::WrongPackage {
                    image: image.name.clone(),
                    bin: bin.clone(),
                });
            }
            [] => {
                return Err(ImageError::UnknownBin {
                    image: image.name.clone(),
                    bin: bin.clone(),
                });
            }
            [only] => *only,
            _ => {
                return Err(ImageError::AmbiguousBin {
                    image: image.name.clone(),
                    bin: bin.clone(),
                });
            }
        };
        check_features(image, member, bin)?;
        if !owners.iter().any(|owner| owner.name == member.name) {
            owners.push(member);
        }
    }
    Ok(owners.iter().map(|owner| owner.name.as_str()).collect())
}

fn check_features(image: &ImageConfig, member: &RustMember, bin: &str) -> Result<(), ImageError> {
    let targets = member
        .targets
        .iter()
        .filter(|target| target.name.as_str() == bin && target.kinds.contains(&TargetKind::Bin));
    for target in targets {
        if target.requires_features && target.required_features.is_empty() {
            return Err(ImageError::BinFeaturesUnrecorded {
                image: image.name.clone(),
                bin: bin.to_owned(),
            });
        }
        let missing = target.required_features.iter().find(|required| {
            !required.enabled(
                image.default_features,
                &image.features,
                member.name.as_str(),
            )
        });
        if let Some(required) = missing {
            return Err(ImageError::BinNeedsFeatures {
                image: image.name.clone(),
                bin: bin.to_owned(),
                feature: required.name.clone(),
            });
        }
    }
    Ok(())
}
