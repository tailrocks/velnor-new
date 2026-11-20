// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use toml::{Table, Value};

use super::Workspace;
use crate::error::CiError;

#[derive(Deserialize)]
struct Member {
    manifest_path: PathBuf,
}

#[derive(Deserialize)]
struct Printed {
    build_directory: Option<PathBuf>,
    #[serde(default)]
    packages: Vec<Member>,
}

pub(super) fn refuse(workspace: &Workspace, metadata: &[u8]) -> Result<(), CiError> {
    let printed: Printed = serde_json::from_slice(metadata)
        .map_err(|e| CiError::new(format!("cargo metadata printed no workspace: {e}")))?;
    for member in &printed.packages {
        let mut directory = member.manifest_path.parent();
        while let Some(current) = directory.filter(|current| *current != workspace.workspace_root) {
            refuse_configuration(current)?;
            directory = current.parent();
        }
    }
    match printed.build_directory {
        Some(build) if build != workspace.target_directory => Err(CiError::new(format!(
            "cargo writes intermediate artifacts to {} outside the target directory {}: a build directory set by `build.build-dir` or `CARGO_BUILD_BUILD_DIR` is not supported, so unset it",
            build.display(),
            workspace.target_directory.display()
        ))),
        _ => Ok(()),
    }
}

fn refuse_configuration(directory: &Path) -> Result<(), CiError> {
    let cargo = directory.join(".cargo");
    let Some(file) = ["config", "config.toml"]
        .iter()
        .map(|name| cargo.join(name))
        .find(|file| file.is_file())
    else {
        return Ok(());
    };
    let text = fs::read_to_string(&file)
        .map_err(|e| CiError::new(format!("cannot read {}: {e}", file.display())))?;
    let table: Table = toml::from_str(&text)
        .map_err(|e| CiError::new(format!("cannot parse {}: {e}", file.display())))?;
    if table.contains_key("include") {
        return Err(CiError::new(format!(
            "{} includes another Cargo configuration file for a workspace member: velnor-ci cannot tell whether the included file sets `build.build-dir`, so remove the include",
            file.display()
        )));
    }
    if table
        .get("build")
        .and_then(Value::as_table)
        .is_some_and(|build| build.contains_key("build-dir"))
    {
        return Err(CiError::new(format!(
            "{} sets `build.build-dir` for a workspace member, so a build run there writes intermediate artifacts outside the target directory: a build directory is not supported, so unset it",
            file.display()
        )));
    }
    Ok(())
}

pub(in crate::io) fn refuse_arguments<'a>(
    words: impl IntoIterator<Item = &'a str>,
) -> Result<(), CiError> {
    let mut words = words.into_iter();
    while let Some(word) = words.next() {
        let named = word == "--build-dir"
            || word.starts_with("--build-dir=")
            || word == "-Zbuild-dir"
            || (word == "-Z" && words.next() == Some("build-dir"));
        if named {
            return Err(CiError::new(
                "inputs build does not accept a build directory (`--build-dir` or `-Z build-dir`): velnor-ci caches, prunes and audits the units in the target directory only, so keep the build directory the target directory",
            ));
        }
    }
    Ok(())
}
