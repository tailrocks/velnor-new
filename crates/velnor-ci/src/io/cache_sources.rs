// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::{Path, PathBuf};

use velnor_model::GitRepository;
use velnor_rust::{CONFIG_PATH, LockAdmission, PinnedGit, merge_pinned};

use crate::error::CiError;
use crate::public_probe::{Deadline, VERDICTS_VARIABLE, Verdicts};

use super::public_repo::{Credentials, env_token};

pub(super) struct Admission {
    pub(super) refusal: Option<String>,
    pub(super) log: String,
    pub(super) mismatch: Option<String>,
}

pub(super) fn refusal(
    root: &Path,
    manifests: &[PathBuf],
    api: &str,
    deadline: Deadline,
) -> Result<Admission, CiError> {
    let refused = |path: &Path| Admission {
        refusal: Some(format!(
            "base caches are not saved because dependency sources in {} are not proven public\n",
            path.display()
        )),
        log: String::new(),
        mismatch: None,
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
    let mut git: Vec<PinnedGit> = Vec::new();
    for manifest in manifests {
        let directory = root
            .join(manifest)
            .parent()
            .map(Path::to_path_buf)
            .ok_or_else(|| CiError::new("manifest has no parent"))?;
        let lock = directory.join("Cargo.lock");
        match read_lock(&lock) {
            Some(LockAdmission::Public) => {}
            Some(LockAdmission::ProbeGithub(found)) => merge_pinned(&mut git, found),
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
    let Ok((declared, undeclared)) = declared_only(root, git) else {
        return Ok(refused(&root.join(CONFIG_PATH.as_str())));
    };
    if !undeclared.is_empty() {
        return Ok(Admission {
            refusal: Some(
                undeclared
                    .iter()
                    .map(|pinned| {
                        format!(
                            "::notice::base caches are not saved because git dependency github.com/{}/{} is not declared in [public_sources], so it is not proven public\n",
                            pinned.repository.owner, pinned.repository.name
                        )
                    })
                    .fold(String::new(), |all, line| all + &line),
            ),
            log: String::new(),
            mismatch: None,
        });
    }
    let git = declared;
    let verdicts = std::env::var(VERDICTS_VARIABLE)
        .ok()
        .and_then(|text| Verdicts::parse(&text));
    Ok(probe(
        api,
        &git,
        env_token().as_deref(),
        verdicts.as_ref(),
        deadline,
    ))
}

pub(super) struct UnreadableDeclaration;

pub(super) fn declared_only(
    root: &Path,
    found: Vec<PinnedGit>,
) -> Result<(Vec<PinnedGit>, Vec<PinnedGit>), UnreadableDeclaration> {
    if found.is_empty() {
        return Ok((found, Vec::new()));
    }
    let declared = declared_repositories(root)?;
    Ok(found.into_iter().partition(|pinned| {
        declared
            .iter()
            .any(|known| known.matches(&pinned.repository.owner, &pinned.repository.name))
    }))
}

fn declared_repositories(root: &Path) -> Result<Vec<GitRepository>, UnreadableDeclaration> {
    let path = root.join(CONFIG_PATH.as_str());
    match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Ok(meta) if meta.is_file() => fs::read(&path)
            .ok()
            .and_then(|bytes| velnor_rust::declared_public_sources(&bytes).ok())
            .ok_or(UnreadableDeclaration),
        _ => Err(UnreadableDeclaration),
    }
}

pub(super) fn describe(credentials: Credentials) -> &'static str {
    match credentials {
        Credentials::Token => "authenticated lookup",
        Credentials::None => "unauthenticated lookup",
    }
}

fn probe(
    api: &str,
    git: &[PinnedGit],
    token: Option<&str>,
    verdicts: Option<&Verdicts>,
    deadline: Deadline,
) -> Admission {
    let mut log = Vec::new();
    let mut refusal = Vec::new();
    let mut mismatch = Vec::new();
    for pinned in git {
        let repository = &pinned.repository;
        let name = format!("github.com/{}/{}", repository.owner, repository.name);
        if verdicts.is_some_and(|known| known.proves_public(repository)) {
            log.push(format!(
                "git dependency {name} is public (verdict of this run)\n"
            ));
            continue;
        }
        match super::public_repo::verify(api, pinned, token, deadline) {
            Ok(credentials) => log.push(format!(
                "git dependency {name} is public ({})\n",
                describe(credentials)
            )),
            Err(rejection) => {
                refusal.push(format!(
                    "::notice::base caches are not saved because git dependency {name} is not proven public ({rejection})\n"
                ));
                if rejection.is_private() {
                    mismatch.push(format!(
                        "git dependency {name} is not public ({rejection}), but Cargo.lock locks it as a public git source; make the repository public again or declare it in [private_sources]\n"
                    ));
                }
            }
        }
    }
    Admission {
        refusal: (!refusal.is_empty()).then(|| refusal.concat()),
        log: log.concat(),
        mismatch: (!mismatch.is_empty()).then(|| mismatch.concat()),
    }
}

pub(super) fn read_lock(path: &Path) -> Option<LockAdmission> {
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
