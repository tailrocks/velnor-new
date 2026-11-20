// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::{Path, PathBuf};

use crate::error::CiError;
use crate::escape::escape;

pub(super) struct Sources {
    pub(super) refusal: Option<String>,
    pub(super) private_sources: bool,
}

pub(super) fn cargo_home() -> Option<PathBuf> {
    std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cargo")))
}

fn refused(path: &Path, not_private: Option<&CiError>) -> Sources {
    let why = not_private.map_or_else(String::new, |error| {
        format!(" and the repository is not proven private ({error})")
    });
    Sources {
        refusal: Some(format!(
            "{}\n",
            escape(
                &format!(
                    "base caches are not saved because dependency sources in {} are not proven public{why}",
                    path.display()
                ),
                false,
            )
        )),
        private_sources: false,
    }
}

pub(super) fn refusal(root: &Path, manifests: &[PathBuf]) -> Result<Sources, CiError> {
    if std::env::vars_os().any(|(key, _)| {
        let key = key.to_string_lossy();
        key.starts_with("CARGO_SOURCE_")
            || key.starts_with("CARGO_REGISTRIES_")
            || key == "CARGO_REGISTRY_INDEX"
            || key == "CARGO_BUILD_TARGET_DIR"
    }) {
        return Ok(refused(Path::new("Cargo environment"), None));
    }
    let Some(cargo_home) = cargo_home() else {
        return Ok(refused(Path::new("CARGO_HOME"), None));
    };
    let mut private_sources = false;
    let mut verdict: Option<Result<(), CiError>> = None;
    for manifest in manifests {
        let directory = root
            .join(manifest)
            .parent()
            .map(Path::to_path_buf)
            .ok_or_else(|| CiError::new("manifest has no parent"))?;
        let lock = directory.join("Cargo.lock");
        if !read_public(&lock, velnor_rust::cacheable_lock) {
            if !read_public(&lock, velnor_rust::cacheable_private_lock) {
                return Ok(refused(&lock, None));
            }
            if let Err(error) = verdict.get_or_insert_with(super::private_repo::confirmed) {
                return Ok(refused(&lock, Some(error)));
            }
            private_sources = true;
        }
        for dir in directory
            .ancestors()
            .map(|dir| dir.join(".cargo"))
            .chain([cargo_home.clone()])
        {
            for name in ["config", "config.toml"] {
                let path = dir.join(name);
                match fs::symlink_metadata(&path) {
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                    Ok(meta) if meta.is_file() => {}
                    _ => return Ok(refused(&path, None)),
                }
                if !read_public(&path, velnor_rust::cacheable_config) {
                    return Ok(refused(&path, None));
                }
            }
        }
    }
    Ok(Sources {
        refusal: None,
        private_sources,
    })
}

fn read_public(path: &Path, check: fn(&str) -> Result<bool, velnor_rust::LockError>) -> bool {
    fs::symlink_metadata(path).is_ok_and(|meta| meta.is_file())
        && fs::read_to_string(path)
            .ok()
            .and_then(|text| check(&text).ok())
            .unwrap_or(false)
}
