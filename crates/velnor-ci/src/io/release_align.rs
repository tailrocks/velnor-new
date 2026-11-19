// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};

use crate::cli::AlignArgs;
use crate::error::CiError;
use crate::release_align::{Root, Source, align, workspace_members};

fn read(path: &Path) -> Result<Option<Source>, CiError> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(Some(Source {
            path: path.display().to_string(),
            text,
        })),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
        Err(e) => Err(CiError::new(format!("cannot read {}: {e}", path.display()))),
    }
}

fn required(path: &Path) -> Result<Source, CiError> {
    read(path)?.ok_or_else(|| CiError::new(format!("cannot read {}: not found", path.display())))
}

fn member_dirs(base: &Path, pattern: &str) -> Result<Vec<PathBuf>, CiError> {
    let (parent, wildcard) = match pattern.strip_suffix("/*") {
        Some(parent) => (parent, true),
        None => (pattern, false),
    };
    let plain = !parent.is_empty()
        && !parent.contains(['*', '?', '[', ']', '{', '}'])
        && Path::new(parent)
            .components()
            .all(|part| matches!(part, Component::Normal(_)));
    if !plain {
        return Err(CiError::new(format!(
            "workspace member `{}` is not a plain path or a plain path ending in /*",
            pattern.escape_debug()
        )));
    }
    let dir = base.join(parent);
    if !wildcard {
        return Ok(vec![dir]);
    }
    let mut found = fs::read_dir(&dir)
        .map_err(|e| CiError::new(format!("cannot read {}: {e}", dir.display())))?
        .map(|entry| {
            entry
                .map(|entry| entry.path())
                .map_err(|e| CiError::new(format!("cannot read {}: {e}", dir.display())))
        })
        .collect::<Result<Vec<_>, _>>()?;
    found.retain(|path| path.join("Cargo.toml").is_file());
    found.sort();
    Ok(found)
}

fn load(manifest: &Path) -> Result<Root, CiError> {
    let base = match manifest.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    let root = required(manifest)?;
    let mut manifests = Vec::new();
    for pattern in workspace_members(&root.path, &root.text)? {
        for dir in member_dirs(base, &pattern)? {
            manifests.push(required(&dir.join("Cargo.toml"))?);
        }
    }
    manifests.insert(0, root);
    Ok(Root {
        manifests,
        lock: read(&base.join("Cargo.lock"))?,
    })
}

pub(super) fn run_align(args: &AlignArgs) -> Result<String, CiError> {
    let lead = load(&args.lead)?;
    let follower = load(&args.follower)?;
    let aligned = align(&lead, &args.lead_package, &follower, &args.members)?;
    for edit in &aligned.edits {
        fs::write(&edit.path, &edit.text)
            .map_err(|e| CiError::new(format!("cannot write {}: {e}", edit.path)))?;
    }
    Ok(aligned.report)
}
