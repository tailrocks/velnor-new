// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_model::{
    HelperRelease, ImageConfig, ImageHost, ImageOpenssl, MbxStore, RepoFacts, RepoPath, RepoView,
    RepositoryIdentity, RustMember, RustRoot, TargetKind,
};
use velnor_render::{IMAGE_WORKFLOW_PATH, ImageBuild, ImageMbx, ImageWorkflow, render_image};

use crate::{GenerateError, ImageError};

pub(crate) fn image_workflow(
    view: &RepoView,
    facts: &RepoFacts,
    helper: &HelperRelease,
    taken: &BTreeMap<RepoPath, String>,
) -> Result<BTreeMap<RepoPath, String>, GenerateError> {
    let images = &facts.config.images;
    if images.is_empty() {
        return Ok(BTreeMap::new());
    }
    if taken.contains_key(&IMAGE_WORKFLOW_PATH) {
        return Err(ImageError::WorkflowTaken.into());
    }
    let RepositoryIdentity::Named(repository) = &view.repository else {
        return Err(ImageError::Unhosted.into());
    };
    if repository.is_empty() {
        return Err(ImageError::Unhosted.into());
    }
    let owner = repository
        .split_once('/')
        .map(|(owner, _)| owner.to_ascii_lowercase());
    let mut builds = Vec::new();
    for image in images {
        check_registries(image, owner.as_deref())?;
        check_tools(facts, image)?;
        check_stage(view, image)?;
        let root = root_of(facts, image)?;
        let package = package_of(image, root)?;
        check_openssl(view, image, root)?;
        builds.push(ImageBuild {
            image,
            root: root_directory(root),
            package,
        });
    }
    let workflow = ImageWorkflow {
        images: builds,
        helper,
        repository,
        branch: &facts.config.default_branch,
        runner: &facts.config.runner,
        private: facts.config.private_sources.as_ref(),
        mbx: mbx_of(facts),
    };
    render_image(&workflow).map_err(GenerateError::Rules)
}

fn mbx_of(facts: &RepoFacts) -> ImageMbx {
    if !facts.config.mbx.routed() {
        ImageMbx::Off
    } else if facts.config.mbx_store == MbxStore::Shared {
        ImageMbx::Shared
    } else {
        ImageMbx::Private
    }
}

fn check_registries(image: &ImageConfig, owner: Option<&str>) -> Result<(), ImageError> {
    for registry in &image.registries {
        if registry.host == ImageHost::Ghcr
            && owner.is_some_and(|owner| {
                registry
                    .repository
                    .split_once('/')
                    .map(|(namespace, _)| namespace)
                    != Some(owner)
            })
        {
            return Err(ImageError::GhcrNamespace {
                image: image.name.clone(),
                repository: registry.repository.clone(),
            });
        }
    }
    Ok(())
}

fn check_tools(facts: &RepoFacts, image: &ImageConfig) -> Result<(), ImageError> {
    for tool in ["zig", "cargo-zigbuild"] {
        let named = |key: &String| key.rsplit(['/', ':']).next() == Some(tool);
        if !facts.tools.tools.keys().any(named) {
            return Err(ImageError::ToolMissing {
                image: image.name.clone(),
                tool,
            });
        }
    }
    Ok(())
}

fn check_stage(view: &RepoView, image: &ImageConfig) -> Result<(), ImageError> {
    let stage = if image.context == "." {
        image.stage.clone()
    } else {
        format!("{}/{}", image.context, image.stage)
    };
    let prefix = format!("{stage}/");
    match view
        .tracked_paths
        .iter()
        .chain(view.files.keys())
        .find(|path| path.as_str().starts_with(&prefix) || path.as_str() == stage)
    {
        Some(path) => Err(ImageError::StageTracked {
            image: image.name.clone(),
            path: path.to_string(),
        }),
        None => Ok(()),
    }
}

fn root_directory(root: &RustRoot) -> &str {
    root.manifest
        .as_str()
        .rsplit_once('/')
        .map_or(".", |(directory, _)| directory)
}

fn root_of<'a>(facts: &'a RepoFacts, image: &ImageConfig) -> Result<&'a RustRoot, ImageError> {
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

fn package_of<'a>(image: &ImageConfig, root: &'a RustRoot) -> Result<Option<&'a str>, ImageError> {
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
        let needs_features = member.targets.iter().any(|target| {
            target.name.as_str() == bin
                && target.kinds.contains(&TargetKind::Bin)
                && target.requires_features
        });
        if needs_features && image.features.is_empty() {
            return Err(ImageError::BinNeedsFeatures {
                image: image.name.clone(),
                bin: bin.clone(),
            });
        }
        if !owners.iter().any(|owner| owner.name == member.name) {
            owners.push(member);
        }
    }
    Ok(match owners.as_slice() {
        [only] => Some(only.name.as_str()),
        _ => None,
    })
}

fn check_openssl(view: &RepoView, image: &ImageConfig, root: &RustRoot) -> Result<(), ImageError> {
    if image.openssl != ImageOpenssl::None {
        return Ok(());
    }
    let lock = match root_directory(root) {
        "." => "Cargo.lock".to_owned(),
        directory => format!("{directory}/Cargo.lock"),
    };
    let holds = RepoPath::new(lock)
        .ok()
        .and_then(|path| view.files.get(&path))
        .is_some_and(|bytes| {
            std::str::from_utf8(bytes)
                .is_ok_and(|text| text.lines().any(|line| line == "name = \"openssl-sys\""))
        });
    if holds {
        Err(ImageError::OpensslRequired {
            image: image.name.clone(),
        })
    } else {
        Ok(())
    }
}
