// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::{Path, PathBuf};

use crate::escape::inert;

mod prune;

use prune::Identity;

const SAVED_ROOT: &str = ".cargo";
const SAVED: &[(&str, bool)] = &[
    ("registry/index", false),
    ("registry/cache", false),
    ("git/db", true),
];
const GIT_DIRECTORY: &str = ".git";
const CREDENTIAL_FILES: &[&str] = &[
    ".git-credentials",
    ".netrc",
    "_netrc",
    "credentials.toml",
    "hosts.yml",
];
const GIT_CREDENTIAL_FILES: &[&str] = &["credentials"];
const GIT_CONFIGS: &[&str] = &["config", "config.worktree", ".gitconfig", "config.bak"];
const HOOKS: &str = "hooks";
const LOGS: &str = "logs";
const FETCH_HEAD: &str = "fetch_head";
const REFS: &str = "refs";
const HEAD: &str = "head";
const CONFIG: &str = "config";
const OBJECTS: &str = "objects";
const OBJECT_INFO: &str = "info";
const OBJECT_PACK: &str = "pack";
const MAX_CONFIG_BYTES: u64 = 1 << 20;
const MAX_DEPTH: usize = 12;
const MAX_ENTRIES: usize = 2_000_000;

#[derive(Clone, Copy, PartialEq)]
enum Context {
    Outside,
    Repository,
    Declared,
    Refs,
    Prunable,
}

impl Context {
    fn entering(self, name: &str, parent_is_git_directory: bool, databases: bool) -> Self {
        if databases || name == GIT_DIRECTORY {
            Self::Declared
        } else if parent_is_git_directory && name == REFS {
            Self::Refs
        } else if parent_is_git_directory && (name == HOOKS || name == LOGS) {
            Self::Prunable
        } else {
            self
        }
    }

    fn git(self) -> bool {
        self != Self::Outside
    }

    fn names_refs(self) -> bool {
        self == Self::Refs
    }
}

pub(super) fn saved_root() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|home| !home.is_empty())
        .map(|home| PathBuf::from(home).join(SAVED_ROOT))
}

pub(super) fn find(root: &Path) -> Option<String> {
    find_within(root, MAX_ENTRIES)
}

fn find_within(root: &Path, entries: usize) -> Option<String> {
    let mut budget = entries;
    for (saved, git) in SAVED {
        let directory = match plain_path(root, saved) {
            Ok(Some(directory)) => directory,
            Ok(None) => continue,
            Err(reason) => return Some(reason),
        };
        let context = if *git {
            Context::Repository
        } else {
            Context::Outside
        };
        if let Some(found) = walk(&directory, 0, &mut budget, context, None) {
            return Some(found);
        }
    }
    None
}

fn plain_path(root: &Path, saved: &str) -> Result<Option<PathBuf>, String> {
    let mut path = root.to_path_buf();
    let mut parts = saved.split('/');
    loop {
        match fs::symlink_metadata(&path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Ok(meta) if meta.is_symlink() => {
                return Err(format!(
                    "{} is a symbolic link that could point a credential outside the saved sources",
                    shown(&path)
                ));
            }
            Ok(meta) if meta.is_dir() => {}
            _ => return Err(format!("{} is not a plain directory", shown(&path))),
        }
        let Some(part) = parts.next() else {
            return Ok(Some(path));
        };
        path.push(part);
    }
}

fn shown(path: &Path) -> String {
    inert(&path.display().to_string())
}

fn charge(budget: &mut usize, directory: &Path) -> Result<(), String> {
    let Some(left) = budget.checked_sub(1) else {
        return Err(format!(
            "{} holds too many entries to scan",
            shown(directory)
        ));
    };
    *budget = left;
    Ok(())
}

fn list(directory: &Path, budget: &mut usize) -> Result<Vec<fs::DirEntry>, String> {
    let entries = fs::read_dir(directory).map_err(|error| {
        format!(
            "cannot read {}: {}",
            shown(directory),
            inert(&error.to_string())
        )
    })?;
    let mut listed = Vec::new();
    for entry in entries {
        charge(budget, directory)?;
        listed.push(entry.map_err(|_| format!("cannot list {}", shown(directory)))?);
    }
    listed.sort_by_key(fs::DirEntry::file_name);
    Ok(listed)
}

fn leave_out(path: &Path, removed: std::io::Result<()>) -> Option<String> {
    removed.err().map(|error| {
        format!(
            "cannot leave out {}: {}",
            shown(path),
            inert(&error.to_string())
        )
    })
}

fn walk(
    directory: &Path,
    depth: usize,
    budget: &mut usize,
    context: Context,
    identity: Option<Identity>,
) -> Option<String> {
    if depth > MAX_DEPTH {
        return Some(format!("{} is nested too deeply to scan", shown(directory)));
    }
    if context == Context::Prunable {
        return match identity {
            Some(identity) => prune::remove_tree(directory, identity, depth, budget),
            None => Some(format!("cannot inspect {}", shown(directory))),
        };
    }
    let listed = match list(directory, budget) {
        Ok(listed) => listed,
        Err(reason) => return Some(reason),
    };
    let databases = depth == 0 && context.git();
    let is_git_directory = context == Context::Declared
        || if context.names_refs() {
            match is_repository_in_refs(directory, &listed, budget) {
                Ok(found) => found,
                Err(reason) => return Some(reason),
            }
        } else {
            is_git_directory(&listed)
        };
    let context = if is_git_directory {
        Context::Repository
    } else {
        context
    };
    let mut subdirectories: Vec<(PathBuf, Context, Option<Identity>)> = Vec::new();
    for entry in listed {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
        let Ok(kind) = entry.file_type() else {
            return Some(format!("cannot inspect {}", shown(&path)));
        };
        if kind.is_symlink() {
            return Some(format!(
                "{} is a symbolic link that could point a credential outside the saved sources",
                shown(&path)
            ));
        }
        let git = context.git() && !context.names_refs();
        if CREDENTIAL_FILES.contains(&name.as_str())
            || (git && GIT_CREDENTIAL_FILES.contains(&name.as_str()))
        {
            return Some(format!("{} is a credential file", shown(&path)));
        }
        if git && kind.is_file() && name == FETCH_HEAD {
            if let Some(found) = leave_out(&path, fs::remove_file(&path)) {
                return Some(found);
            }
        } else if kind.is_dir() {
            let entering = context.entering(&name, is_git_directory, databases);
            let identity = if entering == Context::Prunable {
                match entry.metadata() {
                    Ok(metadata) => Some(Identity::of(&metadata)),
                    Err(_) => return Some(format!("cannot inspect {}", shown(&path))),
                }
            } else {
                None
            };
            subdirectories.push((path, entering, identity));
        } else if !kind.is_file() {
            return Some(format!(
                "{} is not a regular file that can be checked for a credential",
                shown(&path)
            ));
        } else if git && GIT_CONFIGS.contains(&name.as_str()) && holds_credential(&path) {
            return Some(format!(
                "{} holds a credential or a transport setting",
                shown(&path)
            ));
        }
    }
    subdirectories
        .iter()
        .find_map(|(path, context, identity)| walk(path, depth + 1, budget, *context, *identity))
}

fn has_entry(listed: &[fs::DirEntry], wanted: &str, directory: bool) -> bool {
    listed.iter().any(|entry| {
        entry.file_name().eq_ignore_ascii_case(wanted)
            && entry
                .file_type()
                .is_ok_and(|kind| kind.is_dir() == directory && (directory || kind.is_file()))
    })
}

fn is_git_directory(listed: &[fs::DirEntry]) -> bool {
    has_entry(listed, HEAD, false)
        && (has_entry(listed, CONFIG, false) || has_entry(listed, OBJECTS, true))
}

fn is_repository_in_refs(
    directory: &Path,
    listed: &[fs::DirEntry],
    budget: &mut usize,
) -> Result<bool, String> {
    if !has_entry(listed, HEAD, false) {
        return Ok(false);
    }
    Ok(has_object_store(listed, budget)?
        || (has_entry(listed, CONFIG, false) && !is_ref_value(&directory.join(CONFIG))))
}

fn has_object_store(listed: &[fs::DirEntry], budget: &mut usize) -> Result<bool, String> {
    let Some(objects) = listed.iter().find(|entry| {
        entry.file_name().eq_ignore_ascii_case(OBJECTS)
            && entry.file_type().is_ok_and(|kind| kind.is_dir())
    }) else {
        return Ok(false);
    };
    let path = objects.path();
    let Ok(entries) = fs::read_dir(&path) else {
        return Ok(false);
    };
    let mut inner = Vec::new();
    for entry in entries.flatten() {
        charge(budget, &path)?;
        inner.push(entry);
    }
    Ok(has_entry(&inner, OBJECT_INFO, true) && has_entry(&inner, OBJECT_PACK, true))
}

fn is_ref_value(path: &Path) -> bool {
    read_small(path).is_some_and(|text| {
        let value = text.trim_end();
        !value.contains(['\n', '\r'])
            && ((value.starts_with("ref:")
                && !crate::io::saved_git_config::names_credential(value))
                || (matches!(value.len(), 40 | 64)
                    && value.bytes().all(|each| each.is_ascii_hexdigit())))
    })
}

fn holds_credential(path: &Path) -> bool {
    read_small(path).is_none_or(|text| {
        !crate::io::saved_git_config::is_plain(&text)
            || crate::io::saved_git_config::names_credential(&text)
    })
}

fn read_small(path: &Path) -> Option<String> {
    let meta = fs::metadata(path).ok()?;
    if meta.len() > MAX_CONFIG_BYTES {
        return None;
    }
    fs::read_to_string(path).ok()
}

#[cfg(test)]
mod tests;
