// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::{metadata::WorkspaceRecord, metadata_edges::DepKind, release_error::ReleaseError};
use std::{io::Read as _, path::Path};

pub(crate) fn version_declared(
    root: &Path,
    workspace: &WorkspaceRecord,
    package_id: &str,
    alias: &str,
    kind: DepKind,
    target: Option<&str>,
) -> Result<bool, ReleaseError> {
    let fail = || ReleaseError::MetadataMismatch {
        detail: "dependency declaration unavailable or ambiguous".to_owned(),
    };
    let package = workspace
        .packages
        .iter()
        .find(|package| package.id == package_id && package.in_workspace && !package.external)
        .ok_or_else(fail)?;
    let member_manifest = read_manifest(root, &package.manifest)?;
    let parents = match target {
        Some(target) => {
            let expected = target
                .parse::<cargo_platform::Platform>()
                .map_err(|_| fail())?;
            member_manifest
                .get("target")
                .and_then(toml::Value::as_table)
                .ok_or_else(fail)?
                .iter()
                .filter_map(|(name, value)| {
                    name.parse::<cargo_platform::Platform>()
                        .ok()
                        .filter(|actual| *actual == expected)
                        .map(|_| value)
                })
                .collect::<Vec<_>>()
        }
        None => vec![&member_manifest],
    };
    let names: &[&str] = match kind {
        DepKind::Normal => &["dependencies"],
        DepKind::Build => &["build-dependencies", "build_dependencies"],
        DepKind::Dev => &["dev-dependencies", "dev_dependencies"],
    };
    let declarations: Vec<_> = parents
        .into_iter()
        .flat_map(|parent| {
            names
                .iter()
                .filter_map(move |section| parent.get(*section).and_then(|table| table.get(alias)))
        })
        .collect();
    if declarations.is_empty() {
        return Err(fail());
    }
    let mut result = None;
    for declaration in declarations {
        let value = declaration_version(root, workspace, alias, declaration)?;
        if result.is_some_and(|prior| prior != value) {
            return Err(fail());
        }
        result = Some(value);
    }
    result.ok_or_else(fail)
}

fn declaration_version(
    root: &Path,
    workspace: &WorkspaceRecord,
    alias: &str,
    declaration: &toml::Value,
) -> Result<bool, ReleaseError> {
    let fail = || ReleaseError::MetadataMismatch {
        detail: "dependency declaration unavailable or ambiguous".to_owned(),
    };
    if declaration
        .as_table()
        .and_then(|table| table.get("workspace"))
        .and_then(toml::Value::as_bool)
        == Some(true)
    {
        let relative = if workspace.workspace_root.is_empty() {
            "Cargo.toml".to_owned()
        } else {
            format!("{}/Cargo.toml", workspace.workspace_root)
        };
        let root_manifest = read_manifest(root, &relative)?;
        let inherited = root_manifest
            .get("workspace")
            .and_then(|table| table.get("dependencies"))
            .and_then(|table| table.get(alias))
            .ok_or_else(fail)?;
        return has_version(inherited).ok_or_else(fail);
    }
    has_version(declaration).ok_or_else(fail)
}

fn has_version(declaration: &toml::Value) -> Option<bool> {
    if declaration.is_str() {
        return Some(true);
    }
    declaration
        .as_table()
        .map(|table| table.contains_key("version"))
}

fn read_manifest(root: &Path, relative: &str) -> Result<toml::Value, ReleaseError> {
    let fail = || ReleaseError::MetadataMismatch {
        detail: "release dependency manifest invalid".to_owned(),
    };
    let root = root.canonicalize().map_err(|_| fail())?;
    let path = root.join(relative).canonicalize().map_err(|_| fail())?;
    if !path.starts_with(&root) || !path.is_file() {
        return Err(fail());
    }
    let mut content = String::new();
    std::fs::File::open(path)
        .map_err(|_| fail())?
        .take(2 * 1024 * 1024 + 1)
        .read_to_string(&mut content)
        .map_err(|_| fail())?;
    if content.len() > 2 * 1024 * 1024 {
        return Err(fail());
    }
    toml::from_str::<toml::Value>(&content).map_err(|_| fail())
}
