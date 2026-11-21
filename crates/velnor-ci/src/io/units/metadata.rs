// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};
use std::process::Command;

use super::Workspace;
use super::build_dir;
use crate::error::CiError;

pub(in crate::io) struct MetadataInvocation<'a> {
    pub(in crate::io) executable: &'a str,
    pub(in crate::io) arguments: &'a [String],
    pub(in crate::io) directory: &'a Path,
}

pub(in crate::io) fn workspace(
    manifest: &Path,
    invocation: Option<&MetadataInvocation<'_>>,
) -> Result<Workspace, CiError> {
    metadata(manifest, invocation, false)
}

pub(in crate::io) fn own_workspace(
    manifest: &Path,
    invocation: Option<&MetadataInvocation<'_>>,
) -> Result<Workspace, CiError> {
    metadata(manifest, invocation, true)
}

fn metadata(
    manifest: &Path,
    invocation: Option<&MetadataInvocation<'_>>,
    own_target: bool,
) -> Result<Workspace, CiError> {
    let mut command = Command::new(invocation.map_or("cargo", |invocation| invocation.executable));
    if let Some(invocation) = invocation {
        command
            .args(invocation.arguments)
            .current_dir(invocation.directory);
    } else {
        command.current_dir(manifest.parent().unwrap_or(manifest));
    }
    if own_target {
        command
            .env_remove("CARGO_TARGET_DIR")
            .env_remove("CARGO_BUILD_BUILD_DIR");
    } else {
        let base = || match invocation {
            Some(invocation) => Ok(invocation.directory.to_path_buf()),
            None => std::env::current_dir()
                .map_err(|e| CiError::new(format!("cannot read the working directory: {e}"))),
        };
        if let Some(build) = std::env::var_os("CARGO_BUILD_BUILD_DIR").map(PathBuf::from) {
            build_dir::refuse_template(&build)?;
            if build.is_relative() {
                build_dir::refuse_chdir(
                    &build,
                    invocation.map_or(&[], |invocation| invocation.arguments),
                )?;
                build_dir::refuse_parent(&build)?;
                command.env(
                    "CARGO_BUILD_BUILD_DIR",
                    build_dir::step_relative(&build, &base()?),
                );
            }
        }
        if invocation.is_none()
            && let Some(target) = std::env::var_os("CARGO_TARGET_DIR")
                .map(PathBuf::from)
                .filter(|target| target.is_relative())
        {
            command.env(
                "CARGO_TARGET_DIR",
                build_dir::step_relative(&target, &base()?),
            );
        }
    }
    let output = command
        .args([
            "metadata",
            "--format-version",
            "1",
            "--no-deps",
            "--offline",
        ])
        .arg("--manifest-path")
        .arg(manifest)
        .output()
        .map_err(|e| CiError::new(format!("cannot run cargo metadata: {e}")))?;
    if !output.status.success() {
        return Err(CiError::new(format!(
            "cargo metadata failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    let workspace: Workspace = serde_json::from_slice(&output.stdout)
        .map_err(|e| CiError::new(format!("cargo metadata printed no workspace: {e}")))?;
    build_dir::refuse(&workspace, &output.stdout)?;
    Ok(workspace)
}
