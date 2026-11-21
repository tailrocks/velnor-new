// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::{Path, PathBuf};

use crate::escape::inert;

const SAVED_ROOT: &str = ".cargo";
const SAVED: &[(&str, Zone)] = &[
    ("registry/index", Zone::Crate),
    ("registry/cache", Zone::Crate),
    ("git/db", Zone::Databases),
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
const REMOTES: &str = "remotes";
const WORKTREES: &str = "worktrees";
const MODULES: &str = "modules";
const GIT_DIRECTORY_MARKERS: &[&str] = &["head", "config"];
const MAX_CONFIG_BYTES: u64 = 1 << 20;
const MAX_DEPTH: usize = 12;
const MAX_ENTRIES: usize = 2_000_000;

#[derive(Clone, Copy, PartialEq)]
enum Zone {
    Crate,
    Databases,
    Repository,
    Modules,
    Refs,
    Worktrees,
    Plain,
    Hooks,
    Logs,
}

impl Zone {
    fn entering(self, directory: &Path, parent_is_git_directory: bool) -> Self {
        let name = directory.file_name();
        let is = |wanted: &str| name.is_some_and(|name| name.eq_ignore_ascii_case(wanted));
        match self {
            Self::Crate if is(GIT_DIRECTORY) => Self::Repository,
            Self::Databases | Self::Worktrees => Self::Repository,
            Self::Repository => Self::entering_repository(is),
            Self::Modules if parent_is_git_directory && is(HOOKS) => Self::Hooks,
            Self::Modules if parent_is_git_directory && is(LOGS) => Self::Logs,
            Self::Modules if parent_is_git_directory && is(REFS) => Self::Refs,
            Self::Refs if !parent_is(directory, REMOTES) && is_git_directory(directory) => {
                Self::Modules
            }
            other => other,
        }
    }

    fn entering_repository(is: impl Fn(&str) -> bool) -> Self {
        if is(HOOKS) {
            Self::Hooks
        } else if is(LOGS) {
            Self::Logs
        } else if is(MODULES) {
            Self::Modules
        } else if is(GIT_DIRECTORY) {
            Self::Repository
        } else if is(WORKTREES) {
            Self::Worktrees
        } else {
            Self::Plain
        }
    }

    fn git(self) -> bool {
        self != Self::Crate
    }

    fn scans_every_file(self) -> bool {
        matches!(self, Self::Hooks | Self::Logs)
    }
}

pub(super) fn saved_root() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|home| !home.is_empty())
        .map(|home| PathBuf::from(home).join(SAVED_ROOT))
}

pub(super) fn find(root: &Path) -> Option<String> {
    let mut budget = MAX_ENTRIES;
    for (saved, zone) in SAVED {
        let directory = match plain_path(root, saved) {
            Ok(Some(directory)) => directory,
            Ok(None) => continue,
            Err(reason) => return Some(reason),
        };
        if let Some(found) = walk(&directory, 0, &mut budget, *zone) {
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

fn walk(directory: &Path, depth: usize, budget: &mut usize, zone: Zone) -> Option<String> {
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
    let is_git_directory = listed.iter().any(|entry| {
        let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
        GIT_DIRECTORY_MARKERS.contains(&name.as_str())
    });
    let mut subdirectories: Vec<PathBuf> = Vec::new();
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
        let git = zone.git();
        if CREDENTIAL_FILES.contains(&name.as_str())
            || (git && GIT_CREDENTIAL_FILES.contains(&name.as_str()))
        {
            return Some(format!("{} is a credential file", shown(&path)));
        }
        if kind.is_dir() {
            subdirectories.push(path);
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
            && (GIT_TEXT.contains(&name.as_str()) || zone.scans_every_file())
            && names_credential_file(&path, zone)
        {
            return Some(format!(
                "{} names a credential or a transport setting",
                shown(&path)
            ));
        }
    }
    subdirectories.iter().find_map(|path| {
        walk(
            path,
            depth + 1,
            budget,
            zone.entering(path, is_git_directory),
        )
    })
}

fn parent_is(directory: &Path, wanted: &str) -> bool {
    directory
        .parent()
        .and_then(Path::file_name)
        .is_some_and(|name| name.eq_ignore_ascii_case(wanted))
}

fn is_git_directory(directory: &Path) -> bool {
    fs::read_dir(directory).is_ok_and(|entries| {
        entries.flatten().any(|entry| {
            let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
            GIT_DIRECTORY_MARKERS.contains(&name.as_str())
        })
    })
}

fn holds_credential(path: &Path) -> bool {
    read_small(path).is_none_or(|text| {
        !crate::io::saved_git_config::is_plain(&text)
            || crate::io::saved_git_config::names_credential(&text)
    })
}

fn names_credential_file(path: &Path, zone: Zone) -> bool {
    read_small(path).is_none_or(|text| {
        if zone == Zone::Hooks {
            crate::io::saved_git_config::names_hook_credential(&text)
        } else {
            crate::io::saved_git_config::names_credential(&text)
        }
    })
}

fn read_small(path: &Path) -> Option<String> {
    let meta = fs::metadata(path).ok()?;
    if meta.len() > MAX_CONFIG_BYTES {
        return None;
    }
    fs::read_to_string(path).ok()
}
