// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::{Path, PathBuf};

use velnor_model::GitRepository;
use velnor_rust::LockAdmission;

use crate::error::CiError;

pub(super) struct Admission {
    pub(super) refusal: Option<String>,
    pub(super) log: String,
}

pub(super) fn refusal(root: &Path, manifests: &[PathBuf], api: &str) -> Result<Admission, CiError> {
    let refused = |path: &Path| Admission {
        refusal: Some(format!(
            "base caches are not saved because dependency sources in {} are not proven public\n",
            path.display()
        )),
        log: String::new(),
    };
    if std::env::vars_os().any(|(key, _)| {
        let key = key.to_string_lossy();
        key.starts_with("CARGO_SOURCE_")
            || key.starts_with("CARGO_REGISTRIES_")
            || key == "CARGO_REGISTRY_INDEX"
            || key == "CARGO_BUILD_TARGET_DIR"
    }) {
        return Ok(refused(Path::new("Cargo environment")));
    }
    let cargo_home = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cargo")));
    let Some(cargo_home) = cargo_home else {
        return Ok(refused(Path::new("CARGO_HOME")));
    };
    let mut git: Vec<GitRepository> = Vec::new();
    for manifest in manifests {
        let directory = root
            .join(manifest)
            .parent()
            .map(Path::to_path_buf)
            .ok_or_else(|| CiError::new("manifest has no parent"))?;
        let lock = directory.join("Cargo.lock");
        match read_lock(&lock) {
            Some(LockAdmission::Public) => {}
            Some(LockAdmission::ProbeGithub(found)) => {
                for repository in found {
                    if !git.contains(&repository) {
                        git.push(repository);
                    }
                }
            }
            Some(LockAdmission::Refused) | None => return Ok(refused(&lock)),
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
                    _ => return Ok(refused(&path)),
                }
                if !read_public(&path, velnor_rust::cacheable_config) {
                    return Ok(refused(&path));
                }
            }
        }
    }
    Ok(probe(api, &git))
}

fn probe(api: &str, git: &[GitRepository]) -> Admission {
    let mut log = Vec::new();
    let mut refusal = Vec::new();
    for repository in git {
        let name = format!("github.com/{}/{}", repository.owner, repository.name);
        match super::public_repo::check(api, repository) {
            Ok(()) => log.push(format!(
                "git dependency {name} is public (unauthenticated lookup)\n"
            )),
            Err(reason) => {
                let line = format!(
                    "::notice::base caches are not saved because git dependency {name} is not proven public ({reason})\n"
                );
                log.push(line.clone());
                refusal.push(line);
            }
        }
    }
    Admission {
        refusal: (!refusal.is_empty()).then(|| refusal.concat()),
        log: log.concat(),
    }
}

fn read_lock(path: &Path) -> Option<LockAdmission> {
    fs::symlink_metadata(path)
        .is_ok_and(|meta| meta.is_file())
        .then(|| fs::read_to_string(path).ok())
        .flatten()
        .and_then(|text| velnor_rust::lock_admission(&text).ok())
}

fn read_public(path: &Path, check: fn(&str) -> Result<bool, velnor_rust::LockError>) -> bool {
    fs::symlink_metadata(path).is_ok_and(|meta| meta.is_file())
        && fs::read_to_string(path)
            .ok()
            .and_then(|text| check(&text).ok())
            .unwrap_or(false)
}
