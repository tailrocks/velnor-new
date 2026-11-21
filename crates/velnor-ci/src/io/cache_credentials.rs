// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::{Path, PathBuf};

use crate::escape::inert;

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
const GIT_TEXT: &[&str] = &["fetch_head", "packed-refs"];
const HOOKS: &str = "hooks";
const LOGS: &str = "logs";
const REFS: &str = "refs";
const HEAD: &str = "head";
const CONFIG: &str = "config";
const OBJECTS: &str = "objects";
const MAX_CONFIG_BYTES: u64 = 1 << 20;
const MAX_DEPTH: usize = 12;
const MAX_ENTRIES: usize = 2_000_000;

#[derive(Clone, Copy, PartialEq)]
enum Context {
    Outside,
    Repository,
    Database,
    Refs,
    Logs,
}

impl Context {
    fn entering(self, name: &str, parent_is_git_directory: bool, databases: bool) -> Self {
        if databases {
            Self::Database
        } else if name == GIT_DIRECTORY {
            Self::Repository
        } else if parent_is_git_directory && name == REFS {
            Self::Refs
        } else if parent_is_git_directory && name == LOGS {
            Self::Logs
        } else {
            self
        }
    }

    fn git(self) -> bool {
        self != Self::Outside
    }

    fn keeps_hooks(self) -> bool {
        matches!(self, Self::Refs | Self::Logs)
    }
}

pub(super) fn saved_root() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|home| !home.is_empty())
        .map(|home| PathBuf::from(home).join(SAVED_ROOT))
}

pub(super) fn find(root: &Path) -> Option<String> {
    let mut budget = MAX_ENTRIES;
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
        if let Some(found) = walk(&directory, 0, &mut budget, context) {
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

fn walk(directory: &Path, depth: usize, budget: &mut usize, context: Context) -> Option<String> {
    if depth > MAX_DEPTH {
        return Some(format!("{} is nested too deeply to scan", shown(directory)));
    }
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) => {
            return Some(format!(
                "cannot read {}: {}",
                shown(directory),
                inert(&error.to_string())
            ));
        }
    };
    let mut listed = Vec::new();
    for entry in entries {
        let Some(left) = budget.checked_sub(1) else {
            return Some(format!(
                "{} holds too many entries to scan",
                shown(directory)
            ));
        };
        *budget = left;
        let Ok(entry) = entry else {
            return Some(format!("cannot list {}", shown(directory)));
        };
        listed.push(entry);
    }
    listed.sort_by_key(fs::DirEntry::file_name);
    let databases = depth == 0 && context.git();
    let is_git_directory = context == Context::Database || is_git_directory(&listed);
    let context = if is_git_directory {
        Context::Repository
    } else {
        context
    };
    let mut subdirectories: Vec<(PathBuf, Context)> = Vec::new();
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
        let git = context.git();
        if CREDENTIAL_FILES.contains(&name.as_str())
            || (git && GIT_CREDENTIAL_FILES.contains(&name.as_str()))
        {
            return Some(format!("{} is a credential file", shown(&path)));
        }
        if kind.is_dir() {
            if git && name == HOOKS && !context.keeps_hooks() {
                if let Err(error) = fs::remove_dir_all(&path) {
                    return Some(format!(
                        "cannot leave out {}: {}",
                        shown(&path),
                        inert(&error.to_string())
                    ));
                }
                continue;
            }
            subdirectories.push((path, context.entering(&name, is_git_directory, databases)));
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
        } else if git
            && (GIT_TEXT.contains(&name.as_str()) || context == Context::Logs)
            && names_credential_file(&path)
        {
            return Some(format!(
                "{} names a credential or a transport setting",
                shown(&path)
            ));
        }
    }
    subdirectories
        .iter()
        .find_map(|(path, context)| walk(path, depth + 1, budget, *context))
}

fn is_git_directory(listed: &[fs::DirEntry]) -> bool {
    let has = |wanted: &str, directory: bool| {
        listed.iter().any(|entry| {
            entry.file_name().eq_ignore_ascii_case(wanted)
                && entry
                    .file_type()
                    .is_ok_and(|kind| kind.is_dir() == directory && (directory || kind.is_file()))
        })
    };
    has(HEAD, false) && (has(CONFIG, false) || has(OBJECTS, true))
}

fn holds_credential(path: &Path) -> bool {
    read_small(path).is_none_or(|text| {
        !crate::io::saved_git_config::is_plain(&text)
            || crate::io::saved_git_config::names_credential(&text)
    })
}

fn names_credential_file(path: &Path) -> bool {
    read_small(path).is_none_or(|text| crate::io::saved_git_config::names_credential(&text))
}

fn read_small(path: &Path) -> Option<String> {
    let meta = fs::metadata(path).ok()?;
    if meta.len() > MAX_CONFIG_BYTES {
        return None;
    }
    fs::read_to_string(path).ok()
}
