// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageError {
    Unhosted,
    WorkflowTaken,
    DockerfileMissing {
        image: String,
        path: String,
    },
    NotLinuxRunner {
        runner: String,
    },
    UnknownRoot {
        image: String,
        root: String,
    },
    AmbiguousRoot {
        image: String,
    },
    UnknownBin {
        image: String,
        bin: String,
    },
    AmbiguousBin {
        image: String,
        bin: String,
    },
    BinNeedsFeatures {
        image: String,
        bin: String,
        feature: String,
    },
    BinFeaturesUnrecorded {
        image: String,
        bin: String,
    },
    PublishPoolShared {
        image: String,
        runner: String,
    },
    ControlRunnerDiffers {
        first: String,
        second: String,
    },
    WrongPackage {
        image: String,
        bin: String,
    },
    OpensslRequired {
        image: String,
    },
    StageTracked {
        image: String,
        path: String,
    },
    ToolMissing {
        image: String,
        tool: &'static str,
    },
    GhcrNamespace {
        image: String,
        repository: String,
    },
    RegistryShared {
        first: String,
        second: String,
        reference: String,
    },
    TagFamilies {
        first: String,
        second: String,
    },
    BranchTag {
        branch: String,
    },
    BranchReleaseVersion {
        branch: String,
    },
    CpuBaseline {
        image: String,
        path: String,
    },
    DockerfileSymlink {
        image: String,
        path: String,
    },
    SysrootLockMissing {
        image: String,
    },
    ProfileUndefined {
        image: String,
        profile: String,
    },
}

impl fmt::Display for ImageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BinNeedsFeatures { .. }
            | Self::BinFeaturesUnrecorded { .. }
            | Self::PublishPoolShared { .. }
            | Self::ControlRunnerDiffers { .. } => self.feature_or_runner(f),
            Self::Unhosted => f.write_str(
                "[[image]] needs a repository on GitHub: the generated workflow names the repository it may publish from, and this checkout has no origin remote that names one",
            ),
            Self::WorkflowTaken => f.write_str(
                ".github/workflows/image.yml is written by [[image]]; remove the declared workflow that writes it",
            ),
            Self::UnknownRoot { image, root } => write!(
                f,
                "image {image}: root `{root}` is not a Cargo root of this repository"
            ),
            Self::AmbiguousRoot { image } => write!(
                f,
                "image {image}: this repository has several Cargo roots; name one with `root`"
            ),
            Self::UnknownBin { image, bin } => write!(
                f,
                "image {image}: no package of the Cargo root has a binary target named `{bin}`"
            ),
            Self::AmbiguousBin { image, bin } => write!(
                f,
                "image {image}: several packages have a binary target named `{bin}`; name the package with `package`"
            ),
            Self::WrongPackage { image, bin } => write!(
                f,
                "image {image}: binary `{bin}` is not a target of the package named by `package`"
            ),
            Self::OpensslRequired { image } => write!(
                f,
                "image {image}: Cargo.lock holds openssl-sys, which needs OpenSSL for the target; set openssl = \"sysroot\" (the pinned Debian trixie libssl in .velnor/image-sysroot.lock), openssl = \"vendored\" (the repository enables openssl/vendored itself), or drop the native-tls dependency"
            ),
            Self::StageTracked { image, path } => write!(
                f,
                "image {image}: {path} is tracked, but the stage directory receives the compiled binaries; choose another `stage` or untrack the file"
            ),
            Self::ToolMissing { image, tool } => write!(
                f,
                "image {image}: mise.toml and mise.lock must pin `{tool}` for the zigbuild compile"
            ),
            Self::RegistryShared {
                first,
                second,
                reference,
            } => write!(
                f,
                "images {first} and {second} both push to {reference}; two images cannot share a registry repository, because their publish jobs would overwrite each other's tags"
            ),
            Self::TagFamilies { first, second } => write!(
                f,
                "image names {first} and {second} overlap: a tag {second}-v1.2.3 also starts with {first}-v; rename one so neither name plus -v is a prefix of the other"
            ),
            Self::BranchTag { branch } => write!(
                f,
                "the default branch `{branch}` cannot be an image tag (letters, digits, `.`, `_` and `-`, at most 128 characters, no leading `.` or `-`); rename the branch or set default_branch"
            ),
            Self::BranchReleaseVersion { branch } => write!(
                f,
                "the default branch `{branch}` looks like a release version or starts with `sha-`, so its image tag would collide with a tag that a release or a commit owns; rename the branch or set default_branch"
            ),
            Self::CpuBaseline { image, path } => write!(
                f,
                "image {image}: `{path}` sets target-cpu or target-feature in rustflags, which would leak into the published linux/amd64 binary and can crash on another CPU; remove them from that file, or build the image with its own root that does not read it"
            ),
            Self::DockerfileSymlink { image, path } => write!(
                f,
                "image {image}: `{path}` is a symbolic link; the publish job checks out only regular files, so commit the file itself"
            ),
            Self::DockerfileMissing { image, path } => write!(
                f,
                "image {image}: the Dockerfile `{path}` is not a tracked file of this repository"
            ),
            Self::NotLinuxRunner { runner } => write!(
                f,
                "the runner `{runner}` is not a Linux x64 runner; the image jobs build linux/amd64 images with Docker Buildx and install no emulation, so they need an x64 Linux runner with Docker"
            ),
            Self::SysrootLockMissing { image } => write!(
                f,
                "image {image}: openssl = \"sysroot\" needs the tracked file .velnor/image-sysroot.lock"
            ),
            Self::ProfileUndefined { image, profile } => write!(
                f,
                "image {image}: the Cargo root defines no [profile.{profile}]"
            ),
            Self::GhcrNamespace { image, repository } => write!(
                f,
                "image {image}: ghcr.io repository `{repository}` must sit under the lowercase owner of this repository"
            ),
        }
    }
}

impl ImageError {
    fn feature_or_runner(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BinNeedsFeatures {
                image,
                bin,
                feature,
            } => write!(
                f,
                "image {image}: binary `{bin}` requires the feature `{feature}`, which the image does not enable; name it (or a feature that enables it) in `features`, or keep default_features on when a default feature enables it"
            ),
            Self::BinFeaturesUnrecorded { image, bin } => write!(
                f,
                "image {image}: binary `{bin}` has required-features that .velnor/rust-metadata.json does not name; run velnor-gen generate with velnor-ci 0.1.116 or later pinned"
            ),
            Self::PublishPoolShared { image, runner } => write!(
                f,
                "image {image}: `{runner}` is a self-hosted runner shared with the pull request build, so code of a pull request could persist on it and reach the registry credentials of the publish job; set publish_runner to a separate or ephemeral pool (the same label only for a pool whose runners are ephemeral)"
            ),
            Self::ControlRunnerDiffers { first, second } => write!(
                f,
                "images {first} and {second} set different control_runner values, but one gate job serves all images; set the same control_runner on both"
            ),
            _ => Ok(()),
        }
    }
}
