// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::Command;

use serde::Deserialize;
use velnor_model::{RepoPath, RustMetadata};
use velnor_release::Versions;
use velnor_rust::reduce_metadata;

use crate::error::CiError;

const TOOLCHAIN: &str = "RUSTUP_TOOLCHAIN";
const PROGRAM_VARIABLES: &[&str] = &[
    "RUSTC_WRAPPER",
    "RUSTC_WORKSPACE_WRAPPER",
    "CARGO_BUILD_RUSTC_WRAPPER",
    "CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER",
    "CARGO_BUILD_RUSTC",
    "RUSTC",
];

pub(super) struct Workspaces {
    pub(super) metadata: BTreeMap<RepoPath, RustMetadata>,
    pub(super) manifests: BTreeMap<String, String>,
    pub(super) versions: Versions,
}

#[derive(Deserialize)]
struct Listing {
    packages: Vec<Listed>,
}

#[derive(Deserialize)]
struct Listed {
    name: String,
    version: String,
}

pub(super) fn read(top: &Path, roots: &[String]) -> Result<Workspaces, CiError> {
    let toolchain = std::env::var_os(TOOLCHAIN)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            CiError::new(format!(
                "{TOOLCHAIN} is not set, so the checkout could pick the toolchain cargo metadata runs"
            ))
        })?;
    let repository = top
        .canonicalize()
        .map_err(|e| CiError::new(format!("cannot resolve the working folder: {e}")))?;
    let mut metadata = BTreeMap::new();
    let mut versions = Versions::new();
    for root in roots {
        let manifest = if root == "." {
            "Cargo.toml".to_owned()
        } else {
            format!("{root}/Cargo.toml")
        };
        let mut command = Command::new("cargo");
        command
            .args([
                "metadata",
                "--format-version",
                "1",
                "--no-deps",
                "--offline",
            ])
            .arg("--manifest-path")
            .arg(repository.join(&manifest))
            .current_dir(&repository)
            .env(TOOLCHAIN, &toolchain);
        for variable in PROGRAM_VARIABLES.iter().copied() {
            command.env_remove(variable);
        }
        let output = command
            .output()
            .map_err(|e| CiError::new(format!("cannot run cargo metadata for {manifest}: {e}")))?;
        if !output.status.success() {
            return Err(CiError::new(format!(
                "cargo metadata failed for {manifest}: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        let (workspace, packages) = reduce_metadata(&repository, &output.stdout)
            .map_err(|e| CiError::new(format!("{manifest}: {e}")))?;
        let listing: Listing = serde_json::from_slice(&output.stdout)
            .map_err(|e| CiError::new(format!("{manifest}: cargo metadata is not JSON: {e}")))?;
        for listed in listing.packages {
            versions.insert((workspace.to_string(), listed.name), listed.version);
        }
        metadata.insert(workspace, packages);
    }
    let mut manifests = BTreeMap::new();
    for (workspace, packages) in &metadata {
        let paths = std::iter::once(workspace).chain(packages.members.iter().map(|m| &m.manifest));
        for path in paths {
            let text = fs::read_to_string(repository.join(path.as_str()))
                .map_err(|e| CiError::new(format!("cannot read {path}: {e}")))?;
            manifests.insert(path.to_string(), text);
        }
    }
    Ok(Workspaces {
        metadata,
        manifests,
        versions,
    })
}
