// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use super::delta::{failure, safe_path};
use super::delta_source::SourceSnapshot;
use crate::error::CiError;

pub(super) fn invalidate(target: &Path, saved: &SourceSnapshot) -> Result<usize, CiError> {
    let directory = target
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let root = fs::canonicalize(directory).map_err(|e| failure(directory, &e))?;
    let text = fs::read_to_string(target.join(crate::inputs::inputs_file(
        &super::input_scopes::git_prefix(&root)?,
    )))
    .map_err(|e| failure(target, &e))?;
    if super::input_snapshot::decode(&text)?.source_identity != saved.identity {
        return Err(CiError::new(
            "delta sources differ from the saved input identity",
        ));
    }
    let before = saved.records()?;
    let tracked = super::inputs::source_files(&root)?;
    let current = SourceSnapshot {
        identity: super::delta_source::identity_of(&tracked)?,
        tracked,
    };
    if before == current.records()? {
        return Ok(0);
    }
    let external = external_packages(&root);
    let fingerprints = fingerprints(target, &external)?;
    let removed = fingerprints.len();
    for path in fingerprints {
        safe_path(&path)?;
        super::delta_tree::remove_node(&path)?;
    }
    Ok(removed)
}

/// Names of the locked packages that come from a registry or git source and have no path
/// package of the same name. Their sources are immutable for the locked version, so a change to
/// the tracked sources cannot make their fingerprints stale; cargo still rebuilds one whose
/// dependency was rebuilt. A missing or unreadable lockfile yields no names, which keeps every
/// fingerprint invalidated.
fn external_packages(root: &Path) -> BTreeSet<String> {
    let Ok(text) = fs::read_to_string(root.join("Cargo.lock")) else {
        return BTreeSet::new();
    };
    let Ok(lock) = text.parse::<toml::Table>() else {
        return BTreeSet::new();
    };
    let (mut external, mut local) = (BTreeSet::new(), BTreeSet::new());
    for package in lock
        .get("package")
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
    {
        let Some(name) = package.get("name").and_then(toml::Value::as_str) else {
            return BTreeSet::new();
        };
        match package.get("source").and_then(toml::Value::as_str) {
            Some(source)
                if ["registry+", "sparse+", "git+"]
                    .iter()
                    .any(|kind| source.starts_with(kind)) =>
            {
                external.insert(name.to_owned());
            }
            _ => {
                local.insert(name.to_owned());
            }
        }
    }
    &external - &local
}

/// The package that owns a `.fingerprint` directory named `<package>-<16 hex digits>`.
fn fingerprint_package(name: &std::ffi::OsStr) -> Option<&str> {
    let (package, hash) = name.to_str()?.rsplit_once('-')?;
    (hash.len() == 16 && hash.bytes().all(|byte| byte.is_ascii_hexdigit())).then_some(package)
}

fn fingerprints(target: &Path, external: &BTreeSet<String>) -> Result<Vec<PathBuf>, CiError> {
    let mut result = Vec::new();
    for path in super::delta_tree::paths(target)? {
        if path
            .parent()
            .and_then(Path::file_name)
            .is_some_and(|name| name == ".fingerprint")
        {
            if path
                .file_name()
                .and_then(fingerprint_package)
                .is_some_and(|package| external.contains(package))
            {
                continue;
            }
            let full = target.join(&path);
            if fs::symlink_metadata(&full)
                .map_err(|e| failure(&full, &e))?
                .is_dir()
            {
                result.push(full);
            }
        }
    }
    Ok(result)
}
