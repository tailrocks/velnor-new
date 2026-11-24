// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_model::{
    HelperRelease, ImageConfig, ImageHost, ImageOpenssl, MbxStore, MisePlatform, RepoFacts,
    RepoPath, RepoView, RepositoryIdentity, RustMember, RustRoot, TargetKind, ToolPin,
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
    check_all(images, &facts.config.default_branch)?;
    if MisePlatform::runner(facts.config.runner.as_str())
        .is_some_and(|platform| platform.lock_key() != "linux-x64")
    {
        return Err(ImageError::NotLinuxRunner {
            runner: facts.config.runner.to_string(),
        }
        .into());
    }
    let mut builds = Vec::new();
    for image in images {
        check_registries(image, owner.as_deref())?;
        check_tools(facts, image)?;
        check_dockerfile(view, image)?;
        crate::image_inputs::check(view, image)?;
        check_stage(view, image)?;
        let root = root_of(facts, image)?;
        let packages = packages_of(image, root)?;
        check_openssl(view, image, root)?;
        check_sysroot(view, image)?;
        check_profile(view, image, root)?;
        crate::image_cpu::check(view, image, root_directory(root))?;
        builds.push(ImageBuild {
            image,
            root: root_directory(root),
            packages,
        });
    }
    let workflow = ImageWorkflow {
        images: builds,
        helper,
        repository,
        branch: &facts.config.default_branch,
        runner: &facts.config.runner,
        private: facts.config.private_sources.as_ref(),
        mirror: facts
            .config
            .crates_mirror
            .as_ref()
            .filter(|mirror| mirror.runner == facts.config.runner),
        mbx: mbx_of(facts),
        skip_draft_pull_requests: facts.config.skip_draft_pull_requests,
    };
    render_image(&workflow).map_err(GenerateError::Rules)
}

fn check_all(images: &[ImageConfig], branch: &str) -> Result<(), ImageError> {
    let tag = !branch.is_empty()
        && branch.len() <= 128
        && !branch.starts_with(['.', '-'])
        && branch
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'));
    if !tag {
        return Err(ImageError::BranchTag {
            branch: branch.to_owned(),
        });
    }
    let version: Vec<&str> = branch.split('.').collect();
    if branch.starts_with("sha-")
        || version.len() == 3
            && version
                .iter()
                .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
    {
        return Err(ImageError::BranchReleaseVersion {
            branch: branch.to_owned(),
        });
    }
    for (index, image) in images.iter().enumerate() {
        for other in &images[index + 1..] {
            for (first, second) in [(image, other), (other, image)] {
                if second.name.starts_with(&format!("{}-v", first.name)) {
                    return Err(ImageError::TagFamilies {
                        first: first.name.clone(),
                        second: second.name.clone(),
                    });
                }
            }
            for registry in &image.registries {
                let reference = registry.reference();
                if other
                    .registries
                    .iter()
                    .any(|candidate| candidate.reference() == reference)
                {
                    return Err(ImageError::RegistryShared {
                        first: image.name.clone(),
                        second: other.name.clone(),
                        reference,
                    });
                }
            }
        }
    }
    Ok(())
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
    let platform = MisePlatform::runner(facts.config.runner.as_str())
        .unwrap_or_else(|| MisePlatform::new("linux", "x64"));
    for tool in ["zig", "cargo-zigbuild"] {
        let named = |key: &String| key.rsplit(['/', ':']).next() == Some(tool);
        let applies = |pin: &ToolPin| {
            pin.requested
                .iter()
                .any(|request| request.applies_on(platform))
        };
        if !facts
            .tools
            .tools
            .iter()
            .any(|(key, pin)| named(key) && applies(pin))
        {
            return Err(ImageError::ToolMissing {
                image: image.name.clone(),
                tool,
            });
        }
    }
    Ok(())
}

fn check_dockerfile(view: &RepoView, image: &ImageConfig) -> Result<(), ImageError> {
    let tracked = RepoPath::new(image.dockerfile.as_str())
        .is_ok_and(|path| view.tracked_paths.contains(&path) || view.files.contains_key(&path));
    if !tracked {
        return Err(ImageError::DockerfileMissing {
            image: image.name.clone(),
            path: image.dockerfile.clone(),
        });
    }
    let ignore = if image.context == "." {
        ".dockerignore".to_owned()
    } else {
        format!("{}/.dockerignore", image.context)
    };
    let specific = format!("{}.dockerignore", image.dockerfile);
    for path in [image.dockerfile.clone(), ignore, specific] {
        if RepoPath::new(path.as_str()).is_ok_and(|checked| view.symlinks.contains(&checked)) {
            return Err(ImageError::DockerfileSymlink {
                image: image.name.clone(),
                path,
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
    let ancestors: Vec<&str> = stage
        .match_indices('/')
        .map(|(at, _)| &stage[..at])
        .collect();
    match view
        .tracked_paths
        .iter()
        .chain(view.files.keys())
        .find(|path| {
            path.as_str().starts_with(&prefix)
                || path.as_str() == stage
                || ancestors.contains(&path.as_str())
        }) {
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

fn packages_of<'a>(image: &ImageConfig, root: &'a RustRoot) -> Result<Vec<&'a str>, ImageError> {
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
    Ok(owners.iter().map(|owner| owner.name.as_str()).collect())
}

fn check_profile(view: &RepoView, image: &ImageConfig, root: &RustRoot) -> Result<(), ImageError> {
    if matches!(image.profile.as_str(), "release" | "dev") {
        return Ok(());
    }
    let defined = view
        .files
        .get(&root.manifest)
        .and_then(|bytes| std::str::from_utf8(bytes).ok())
        .and_then(|text| toml::from_str::<toml::Table>(text).ok())
        .and_then(|manifest| {
            manifest
                .get("profile")?
                .as_table()?
                .get(&image.profile)
                .map(toml::Value::is_table)
        })
        .unwrap_or(false);
    if defined {
        Ok(())
    } else {
        Err(ImageError::ProfileUndefined {
            image: image.name.clone(),
            profile: image.profile.clone(),
        })
    }
}

fn check_sysroot(view: &RepoView, image: &ImageConfig) -> Result<(), ImageError> {
    let lock = RepoPath::new(".velnor/image-sysroot.lock").ok();
    let present = lock
        .is_some_and(|path| view.files.contains_key(&path) || view.tracked_paths.contains(&path));
    if image.openssl == ImageOpenssl::Sysroot && !present {
        return Err(ImageError::SysrootLockMissing {
            image: image.name.clone(),
        });
    }
    Ok(())
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
                .ok()
                .and_then(|text| toml::from_str::<toml::Table>(text).ok())
                .and_then(|lock| lock.get("package")?.as_array().cloned())
                .is_some_and(|packages| {
                    packages.iter().any(|package| {
                        package.get("name").and_then(toml::Value::as_str) == Some("openssl-sys")
                    })
                })
        });
    if holds {
        Err(ImageError::OpensslRequired {
            image: image.name.clone(),
        })
    } else {
        Ok(())
    }
}
