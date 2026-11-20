// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::io::ErrorKind;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};

use crate::error::CiError;

const CONFIGS: &[&str] = &["config", "config.worktree"];
const NEVER_COPIED: &[&str] = &[
    "config",
    "config.worktree",
    "hooks",
    "alternates",
    "http-alternates",
];
const BARE: &str = "[core]\n\trepositoryformatversion = 0\n\tfilemode = true\n\tbare = true\n";
const WORKING: &str = "[core]\n\trepositoryformatversion = 0\n\tfilemode = true\n\tbare = false\n\tlogallrefupdates = true\n";

pub(super) fn check_databases(cache: &Path) -> Result<(), CiError> {
    for database in children(&cache.join("git/db"))? {
        if !fs::metadata(&database).is_ok_and(|meta| meta.is_dir()) {
            continue;
        }
        let mut configs: Vec<PathBuf> = CONFIGS.iter().map(|name| database.join(name)).collect();
        configs.extend(
            children(&database.join("worktrees"))?
                .into_iter()
                .map(|worktree| worktree.join("config.worktree")),
        );
        for config in configs {
            match fs::read_to_string(&config) {
                Ok(text) if super::saved_git_config::is_plain(&text) => {}
                Err(e) if e.kind() == ErrorKind::NotFound => {}
                _ => {
                    return Err(CiError::new(format!(
                        "{} is not a plain git config; refusing to run the fetch with the token",
                        config.display()
                    )));
                }
            }
        }
    }
    Ok(())
}

pub(super) fn merge(fetched: &Path, cache: &Path) -> Result<(), CiError> {
    for database in children(&fetched.join("db"))? {
        let target = cache.join("db").join(name(&database)?);
        if target.exists() {
            objects(&database.join("objects"), &target.join("objects"))?;
        } else {
            install(&target, |staged| git_directory(&database, staged, BARE))?;
        }
    }
    for group in children(&fetched.join("checkouts"))? {
        for revision in children(&group)? {
            let target = cache
                .join("checkouts")
                .join(name(&group)?)
                .join(name(&revision)?);
            if !target.exists() {
                install(&target, |staged| working_tree(&revision, staged))?;
            }
        }
    }
    Ok(())
}

fn install(target: &Path, copy: impl FnOnce(&Path) -> Result<(), CiError>) -> Result<(), CiError> {
    let parent = target.parent().unwrap_or(target);
    fs::create_dir_all(parent).map_err(|e| failed(parent, &e))?;
    let staged = parent.join(format!(".{}.velnor-{}", name(target)?, std::process::id()));
    drop(fs::remove_dir_all(&staged));
    copy(&staged)?;
    fs::rename(&staged, target).or_else(|e| {
        drop(fs::remove_dir_all(&staged));
        if target.exists() {
            Ok(())
        } else {
            Err(failed(target, &e))
        }
    })
}

fn git_directory(source: &Path, target: &Path, config: &str) -> Result<(), CiError> {
    plain_files(source, target)?;
    fs::write(target.join("config"), config).map_err(|e| failed(target, &e))
}

fn plain_files(source: &Path, target: &Path) -> Result<(), CiError> {
    fs::create_dir_all(target).map_err(|e| failed(target, &e))?;
    for entry in children(source)? {
        let name = name(&entry)?;
        if NEVER_COPIED.contains(&name) {
            continue;
        }
        let kind = fs::symlink_metadata(&entry).map_err(|e| failed(&entry, &e))?;
        if kind.is_dir() {
            plain_files(&entry, &target.join(name))?;
        } else if kind.is_file() {
            fs::copy(&entry, target.join(name)).map_err(|e| failed(&entry, &e))?;
        }
    }
    Ok(())
}

fn working_tree(source: &Path, target: &Path) -> Result<(), CiError> {
    fs::create_dir_all(target).map_err(|e| failed(target, &e))?;
    for entry in children(source)? {
        let name = name(&entry)?;
        let destination = target.join(name);
        let kind = fs::symlink_metadata(&entry).map_err(|e| failed(&entry, &e))?;
        if kind.is_dir() && name == ".git" {
            git_directory(&entry, &destination, WORKING)?;
        } else if kind.is_dir() {
            working_tree(&entry, &destination)?;
        } else if kind.is_symlink() {
            let link = fs::read_link(&entry).map_err(|e| failed(&entry, &e))?;
            symlink(link, &destination).map_err(|e| failed(&destination, &e))?;
        } else if kind.is_file() {
            fs::copy(&entry, &destination).map_err(|e| failed(&entry, &e))?;
        }
    }
    Ok(())
}

fn objects(source: &Path, target: &Path) -> Result<(), CiError> {
    let mut entries = children(source)?;
    entries.sort_by_key(|entry| {
        entry
            .extension()
            .is_some_and(|extension| extension == "idx")
    });
    for entry in entries {
        let name = name(&entry)?;
        let destination = target.join(name);
        let kind = fs::symlink_metadata(&entry).map_err(|e| failed(&entry, &e))?;
        if NEVER_COPIED.contains(&name) || name == "info" {
            continue;
        }
        if kind.is_dir() {
            fs::create_dir_all(&destination).map_err(|e| failed(&destination, &e))?;
            objects(&entry, &destination)?;
        } else if kind.is_file() && !destination.exists() {
            let staged = target.join(format!(".{name}.velnor-{}", std::process::id()));
            fs::copy(&entry, &staged)
                .and_then(|_| fs::rename(&staged, &destination))
                .map_err(|e| failed(&destination, &e))?;
        }
    }
    Ok(())
}

fn children(directory: &Path) -> Result<Vec<PathBuf>, CiError> {
    match fs::read_dir(directory) {
        Ok(entries) => entries
            .map(|entry| {
                entry
                    .map(|entry| entry.path())
                    .map_err(|e| failed(directory, &e))
            })
            .collect(),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(failed(directory, &e)),
    }
}

fn name(path: &Path) -> Result<&str, CiError> {
    path.file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| CiError::new(format!("{} has no plain name", path.display())))
}

fn failed(path: &Path, error: &std::io::Error) -> CiError {
    CiError::new(format!(
        "cannot move the fetched git sources at {}: {error}",
        path.display()
    ))
}
