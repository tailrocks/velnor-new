// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::{Path, PathBuf};

const SAVED_ROOT: &str = ".cargo";
const SAVED: [&str; 3] = ["registry/index", "registry/cache", "git/db"];
const CREDENTIAL_FILES: [&str; 6] = [
    ".git-credentials",
    ".netrc",
    "_netrc",
    "credentials",
    "credentials.toml",
    "hosts.yml",
];
const GIT_CONFIG: &str = "config";
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
            _ => return Some(format!("{} is not a plain directory", directory.display())),
        }
        if let Some(found) = walk(&directory, 0, &mut budget) {
            return Some(found);
        }
    }
    None
}

fn walk(directory: &Path, depth: usize, budget: &mut usize) -> Option<String> {
    if depth > MAX_DEPTH {
        return Some(format!(
            "{} is nested too deeply to scan",
            directory.display()
        ));
    }
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) => return Some(format!("cannot read {}: {error}", directory.display())),
    };
    let mut subdirectories: Vec<PathBuf> = Vec::new();
    for entry in entries {
        let Some(left) = budget.checked_sub(1) else {
            return Some(format!(
                "{} holds too many entries to scan",
                directory.display()
            ));
        };
        *budget = left;
        let Ok(entry) = entry else {
            return Some(format!("cannot list {}", directory.display()));
        };
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
        let Ok(kind) = entry.file_type() else {
            return Some(format!("cannot inspect {}", path.display()));
        };
        if CREDENTIAL_FILES.contains(&name.as_str()) {
            return Some(format!("{} is a credential file", path.display()));
        }
        if kind.is_dir() {
            subdirectories.push(path);
        } else if kind.is_file() && name == GIT_CONFIG && holds_credential(&path) {
            return Some(format!("{} holds a credential", path.display()));
        }
    }
    subdirectories
        .iter()
        .find_map(|path| walk(path, depth + 1, budget))
}

fn holds_credential(path: &Path) -> bool {
    let Ok(meta) = fs::metadata(path) else {
        return true;
    };
    if meta.len() > MAX_CONFIG_BYTES {
        return true;
    }
    let Ok(text) = fs::read_to_string(path) else {
        return true;
    };
    text.lines().any(|line| {
        let line = line.trim().to_ascii_lowercase();
        line.starts_with("extraheader")
            || line.contains("authorization")
            || line.contains("password")
            || line.contains("token")
            || userinfo(&line)
    })
}

fn userinfo(line: &str) -> bool {
    line.match_indices("http").any(|(at, _)| {
        let rest = &line[at..];
        let Some(rest) = rest
            .strip_prefix("https://")
            .or_else(|| rest.strip_prefix("http://"))
        else {
            return false;
        };
        rest.split(['/', ' ', '\t'])
            .next()
            .unwrap_or("")
            .contains('@')
    })
}
