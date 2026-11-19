// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! The workspace crates a shipped binary compiles in (#1628). A change in any
//! of them changes the binary, so release-plz.toml must list each one.

use std::collections::{BTreeMap, BTreeSet};

use velnor_model::{DependencyKind, RepoPath, RustMetadata};

use std::fmt;

use crate::error::ReleaseError;
use crate::release_plz::RootConfig;
use crate::velnor_config::Binary;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShippedError {
    Unlisted { binary: String, package: String },
}

impl fmt::Display for ShippedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unlisted { binary, package } => write!(
                f,
                "release-plz.toml: {package} is compiled into the shipped binary {binary}, so a change in it changes the binary, but no [[package]] lists it; add one (release = false when the family tag releases the binary)"
            ),
        }
    }
}

impl From<ShippedError> for ReleaseError {
    fn from(error: ShippedError) -> Self {
        Self::Shipped(error)
    }
}

/// The members of `workspace` that `binary` compiles in: itself and every
/// member it reaches through normal or build dependencies. Dev-dependencies
/// are not compiled into the binary.
pub(crate) fn shipped_crates(workspace: &RustMetadata, binary: &str) -> BTreeSet<String> {
    let members: BTreeSet<&str> = workspace
        .members
        .iter()
        .map(|member| member.name.as_str())
        .collect();
    let mut reached = BTreeSet::new();
    let mut pending = vec![binary.to_owned()];
    while let Some(name) = pending.pop() {
        if !reached.insert(name.clone()) {
            continue;
        }
        for member in workspace
            .members
            .iter()
            .filter(|member| member.name.as_str() == name)
        {
            for dependency in &member.dependencies {
                let compiled = matches!(
                    dependency.kind,
                    DependencyKind::Normal | DependencyKind::Build
                );
                if compiled && members.contains(dependency.name.as_str()) {
                    pending.push(dependency.name.as_str().to_owned());
                }
            }
        }
    }
    reached
}

/// Every crate a shipped binary compiles in must be listed by some root's
/// release-plz.toml. The binary's own package is checked by `BinaryNotReleased`.
pub(crate) fn check_shipped(
    binaries: &[Binary],
    configs: &[RootConfig],
    metadata: &BTreeMap<RepoPath, RustMetadata>,
) -> Result<(), ShippedError> {
    for binary in binaries {
        let Some(workspace) = metadata.values().find(|workspace| {
            workspace
                .members
                .iter()
                .any(|member| member.name.as_str() == binary.package)
        }) else {
            continue;
        };
        for name in shipped_crates(workspace, &binary.package) {
            if name != binary.package && !configs.iter().any(|config| config.lists(&name)) {
                return Err(ShippedError::Unlisted {
                    binary: binary.package.clone(),
                    package: name,
                });
            }
        }
    }
    Ok(())
}
