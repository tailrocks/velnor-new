// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::io::ErrorKind;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};

use crate::error::CiError;

const NEVER_COPIED: &[&str] = &[
    "config",
    "config.worktree",
    "hooks",
    "alternates",
    "http-alternates",
];
const BARE: &str = "[core]\n\trepositoryformatversion = 0\n\tfilemode = true\n\tbare = true\n";
const WORKING: &str = "[core]\n\trepositoryformatversion = 0\n\tfilemode = true\n\tbare = false\n\tlogallrefupdates = true\n";

pub(super) fn merge(fetched: &Path, cache: &Path) -> Result<(), CiError> {
    for directory in [
        cache.to_path_buf(),
        cache.join("db"),
        cache.join("checkouts"),
    ] {
        unlinked(&directory)?;
    }
    for database in children(&fetched.join("db"))? {
        let target = cache.join("db").join(name(&database)?);
        unlinked(&target)?;
        if target.exists() {
            objects(&database.join("objects"), &target.join("objects"))?;
        } else {
            install(&target, |staged| git_directory(&database, staged, BARE))?;
        }
    }
    for group in children(&fetched.join("checkouts"))? {
        let checkouts = cache.join("checkouts").join(name(&group)?);
        unlinked(&checkouts)?;
        for revision in children(&group)? {
            let target = checkouts.join(name(&revision)?);
            unlinked(&target)?;
            if !target.exists() {
                install(&target, |staged| working_tree(&revision, staged))?;
            } else if !complete(&target) && complete(&revision) {
                replace(&target, |staged| working_tree(&revision, staged))?;
            }
        }
    }
    Ok(())
}

fn unlinked(path: &Path) -> Result<(), CiError> {
    if fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_symlink()) {
        return Err(CiError::new(format!(
            "{} is a symbolic link; refusing to copy the fetched git sources through it",
            path.display()
        )));
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

fn complete(checkout: &Path) -> bool {
    fs::symlink_metadata(checkout.join(".cargo-ok")).is_ok_and(|meta| meta.is_file())
}

fn replace(target: &Path, copy: impl FnOnce(&Path) -> Result<(), CiError>) -> Result<(), CiError> {
    let aside = target.with_file_name(format!(
        ".{}.velnor-old-{}",
        name(target)?,
        std::process::id()
    ));
    drop(fs::remove_dir_all(&aside));
    fs::rename(target, &aside).map_err(|e| failed(target, &e))?;
    let installed = install(target, copy);
    if installed.is_err() && !target.exists() {
        drop(fs::rename(&aside, target));
    } else {
        drop(fs::remove_dir_all(&aside));
    }
    installed
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
    unlinked(target)?;
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
            drop(fs::remove_file(&staged));
            stage(&entry, &staged)
                .and_then(|()| fs::rename(&staged, &destination))
                .map_err(|e| failed(&destination, &e))?;
        }
    }
    Ok(())
}

fn stage(source: &Path, staged: &Path) -> std::io::Result<()> {
    let mut input = fs::File::open(source)?;
    let mut output = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(staged)?;
    std::io::copy(&mut input, &mut output).map(drop)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("velnor-git-{name}-{}", std::process::id()));
        drop(fs::remove_dir_all(&root));
        fs::create_dir_all(&root).unwrap_or_else(|e| unreachable!("{e}"));
        fs::canonicalize(&root).unwrap_or_else(|e| unreachable!("{e}"))
    }

    fn put(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap_or_else(|| unreachable!()))
            .and_then(|()| fs::write(path, text))
            .unwrap_or_else(|e| unreachable!("{e}"));
    }

    #[test]
    fn a_cached_checkout_without_the_completion_marker_is_replaced_by_the_fetched_one() {
        let root = tree("incomplete");
        let (fetched, cache) = (root.join("fetched"), root.join("cache"));
        let revision = "checkouts/dep-1/abc";
        put(&fetched.join(revision).join("lib.rs"), "whole");
        put(&fetched.join(revision).join(".cargo-ok"), "{}");
        put(&cache.join(revision).join("lib.rs"), "trunc");
        put(&cache.join(revision).join("stale.rs"), "stale");
        merge(&fetched, &cache).unwrap_or_else(|e| unreachable!("{e}"));
        let kept = cache.join(revision);
        assert_eq!(
            fs::read_to_string(kept.join("lib.rs")).ok().as_deref(),
            Some("whole")
        );
        assert!(kept.join(".cargo-ok").is_file());
        assert!(!kept.join("stale.rs").exists());
        assert_eq!(
            children(&cache.join("checkouts/dep-1"))
                .map(|c| c.len())
                .ok(),
            Some(1)
        );
    }

    #[test]
    fn a_complete_cached_checkout_is_left_alone() {
        let root = tree("complete");
        let (fetched, cache) = (root.join("fetched"), root.join("cache"));
        let revision = "checkouts/dep-1/abc";
        put(&fetched.join(revision).join("lib.rs"), "fetched");
        put(&fetched.join(revision).join(".cargo-ok"), "{}");
        put(&cache.join(revision).join("lib.rs"), "cached");
        put(&cache.join(revision).join(".cargo-ok"), "{}");
        merge(&fetched, &cache).unwrap_or_else(|e| unreachable!("{e}"));
        let text = fs::read_to_string(cache.join(revision).join("lib.rs"));
        assert_eq!(text.ok().as_deref(), Some("cached"));
    }

    #[test]
    fn a_linked_objects_directory_in_a_cached_database_is_never_written_through() {
        for linked in ["objects", "objects/pack"] {
            let root = tree(&format!("objects-{}", linked.replace('/', "-")));
            let (fetched, cache) = (root.join("fetched"), root.join("cache"));
            let outside = root.join("outside");
            fs::create_dir_all(&outside).unwrap_or_else(|e| unreachable!("{e}"));
            put(&fetched.join("db/dep-1/objects/pack/pack-1.pack"), "data");
            put(&fetched.join("db/dep-1/objects/ab/cdef"), "loose");
            put(&cache.join("db/dep-1/HEAD"), "ref");
            let link = cache.join("db/dep-1").join(linked);
            fs::create_dir_all(link.parent().unwrap_or_else(|| unreachable!()))
                .unwrap_or_else(|e| unreachable!("{e}"));
            symlink(&outside, &link).unwrap_or_else(|e| unreachable!("{e}"));
            let error = merge(&fetched, &cache).err().map(|e| e.to_string());
            assert_eq!(
                fs::read_dir(&outside).map(Iterator::count).ok(),
                Some(0),
                "{linked}"
            );
            assert!(
                error.is_some_and(|e| e.contains("symbolic link")),
                "{linked}"
            );
        }
    }
}
