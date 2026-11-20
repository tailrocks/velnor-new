// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use crate::error::CiError;

pub(super) fn resolve(link: &Path, checkout: &Path) -> Result<PathBuf, CiError> {
    let resolved = fs::canonicalize(link).map_err(|e| {
        CiError::new(format!(
            "cache target {} is a symlink that cannot be resolved ({e}); remove it or let the build recreate it",
            link.display()
        ))
    })?;
    let metadata = fs::metadata(&resolved)
        .map_err(|e| CiError::new(format!("cannot inspect {}: {e}", resolved.display())))?;
    if !metadata.is_dir() {
        return Err(CiError::new(format!(
            "cache target {} is a symlink to {}, which is not a directory",
            link.display(),
            resolved.display()
        )));
    }
    let owner = nix::unistd::geteuid().as_raw();
    if metadata.uid() != owner {
        return Err(CiError::new(format!(
            "cache target {} is a symlink to {}, which is owned by another user",
            link.display(),
            resolved.display()
        )));
    }
    let roots = allowed_roots(checkout);
    if roots
        .iter()
        .any(|root| resolved != *root && resolved.starts_with(root))
    {
        return Ok(resolved);
    }
    let reason = if roots.contains(&resolved) {
        "not below"
    } else {
        "outside"
    };
    Err(CiError::new(format!(
        "cache target {} is a symlink to {}, which is {reason} the mbx cache, RUNNER_TEMP and the checkout; refusing to follow it",
        link.display(),
        resolved.display()
    )))
}

fn allowed_roots(checkout: &Path) -> Vec<PathBuf> {
    let var = |name: &str| std::env::var_os(name).filter(|value| !value.is_empty());
    let home = var("HOME").map(PathBuf::from);
    let cache_dirs: Vec<PathBuf> = match var("MBX_CACHE_DIR") {
        Some(dir) => vec![PathBuf::from(dir)],
        None => var("XDG_CACHE_HOME")
            .map(|dir| PathBuf::from(dir).join("mbx"))
            .into_iter()
            .chain(home.iter().map(|home| home.join(".cache/mbx")))
            .chain(home.iter().map(|home| home.join("Library/Caches/mbx")))
            .collect(),
    };
    let target_root = var("MBX_TARGET_ROOT").map(PathBuf::from).map(|root| {
        cache_dirs
            .first()
            .map_or_else(|| root.clone(), |cache| cache.join(&root))
    });
    let mut roots = vec![checkout.to_path_buf()];
    roots.extend(var("RUNNER_TEMP").map(PathBuf::from));
    roots.extend(cache_dirs.into_iter().map(|dir| dir.join("targets")));
    roots.extend(target_root);
    let mut roots: Vec<PathBuf> = roots
        .into_iter()
        .filter(|root| root.is_absolute())
        .filter_map(|root| fs::canonicalize(root).ok())
        .collect();
    roots.sort();
    roots.dedup();
    roots
}
