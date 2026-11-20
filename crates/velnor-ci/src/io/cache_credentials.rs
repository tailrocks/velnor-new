// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::{Path, PathBuf};

use crate::escape::inert;

const SAVED_ROOT: &str = ".cargo";
const SAVED: &[&str] = &["registry/index", "registry/cache", "git/db"];
const CREDENTIAL_FILES: &[&str] = &[
    ".git-credentials",
    ".netrc",
    "_netrc",
    "credentials",
    "credentials.toml",
    "hosts.yml",
];
const GIT_CONFIGS: &[&str] = &["config", "config.worktree", ".gitconfig", "config.bak"];
const GIT_TEXT: &[&str] = &["fetch_head", "packed-refs"];
const HOOKS: &str = "hooks";
const MAX_CONFIG_BYTES: u64 = 1 << 20;
const MAX_DEPTH: usize = 12;
const MAX_ENTRIES: usize = 2_000_000;

pub(super) fn saved_root() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|home| !home.is_empty())
        .map(|home| PathBuf::from(home).join(SAVED_ROOT))
}

pub(super) fn find(root: &Path) -> Option<String> {
    let mut budget = MAX_ENTRIES;
    for saved in SAVED {
        let directory = root.join(saved);
        match fs::symlink_metadata(&directory) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Ok(meta) if meta.is_dir() => {}
            _ => return Some(format!("{} is not a plain directory", shown(&directory))),
        }
        if let Some(found) = walk(&directory, 0, &mut budget) {
            return Some(found);
        }
    }
    None
}

fn shown(path: &Path) -> String {
    inert(&path.display().to_string())
}

fn walk(directory: &Path, depth: usize, budget: &mut usize) -> Option<String> {
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
    let mut subdirectories: Vec<PathBuf> = Vec::new();
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
        if CREDENTIAL_FILES.contains(&name.as_str()) {
            return Some(format!("{} is a credential file", shown(&path)));
        }
        if kind.is_dir() {
            subdirectories.push(path);
        } else if !kind.is_file() {
            return Some(format!(
                "{} is not a regular file that can be checked for a credential",
                shown(&path)
            ));
        } else if GIT_CONFIGS.contains(&name.as_str()) && holds_credential(&path) {
            return Some(format!(
                "{} holds a credential or a transport setting",
                shown(&path)
            ));
        } else if (GIT_TEXT.contains(&name.as_str()) || is_hooks(directory))
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
        .find_map(|path| walk(path, depth + 1, budget))
}

fn is_hooks(directory: &Path) -> bool {
    directory.file_name().is_some_and(|name| name == HOOKS)
}

fn holds_credential(path: &Path) -> bool {
    read_small(path).is_none_or(|text| !crate::io::saved_git_config::is_plain(&text))
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
