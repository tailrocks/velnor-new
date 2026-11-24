// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageError {
    Unhosted,
    WorkflowTaken,
    UnknownRoot { image: String, root: String },
    AmbiguousRoot { image: String },
    UnknownBin { image: String, bin: String },
    AmbiguousBin { image: String, bin: String },
    BinNeedsFeatures { image: String, bin: String },
    WrongPackage { image: String, bin: String },
    OpensslRequired { image: String },
    StageTracked { image: String, path: String },
    ToolMissing { image: String, tool: &'static str },
    GhcrNamespace { image: String, repository: String },
}

impl fmt::Display for ImageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
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
            Self::BinNeedsFeatures { image, bin } => write!(
                f,
                "image {image}: binary `{bin}` has required-features; list the features it needs in `features`"
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
            Self::GhcrNamespace { image, repository } => write!(
                f,
                "image {image}: ghcr.io repository `{repository}` must sit under the lowercase owner of this repository"
            ),
        }
    }
}
