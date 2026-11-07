// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{JournalFile, JournalFileKind, valid_files, valid_path};
use std::collections::BTreeSet;

pub(crate) fn valid_source_files(files: &[JournalFile]) -> bool {
    if !valid_files(files) {
        return false;
    }
    let paths: std::collections::BTreeMap<_, _> = files
        .iter()
        .map(|file| (file.path.as_str(), file))
        .collect();
    files.iter().all(|file| {
        let mut current = file;
        let mut seen = BTreeSet::new();
        loop {
            if !seen.insert(current.path.as_str()) {
                return false;
            }
            match &current.kind {
                JournalFileKind::Regular { .. } => return true,
                JournalFileKind::Symlink { target } => {
                    if crate::planner::generator::sha256_hex(target.as_bytes()) != current.sha256 {
                        return false;
                    }
                    let Some(resolved) = resolve_link(&current.path, target) else {
                        return false;
                    };
                    let Some(next) = paths.get(resolved.as_str()) else {
                        return false;
                    };
                    current = next;
                }
            }
        }
    })
}

fn resolve_link(path: &str, target: &str) -> Option<String> {
    if target.is_empty()
        || target.starts_with('/')
        || target.contains('\\')
        || target.chars().any(char::is_control)
    {
        return None;
    }
    let mut parts: Vec<&str> = path.split('/').collect();
    parts.pop();
    for part in target.split('/') {
        match part {
            "" | "." => return None,
            ".." => {
                parts.pop()?;
            }
            component => parts.push(component),
        }
    }
    let resolved = parts.join("/");
    valid_path(&resolved).then_some(resolved)
}
