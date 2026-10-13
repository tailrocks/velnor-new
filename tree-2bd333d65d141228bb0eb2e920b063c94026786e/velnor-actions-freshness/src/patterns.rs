//! Minimal deterministic expansion for Cargo workspace member globs.

use std::fs;
use std::path::{Path, PathBuf};

/// Expand a slash-separated Cargo member pattern beneath `root`.
pub(crate) fn expand(root: &Path, pattern: &str) -> Result<Vec<PathBuf>, String> {
    let parts = pattern
        .split('/')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    let mut found = Vec::new();
    expand_parts(root, &parts, &mut found)?;
    found.sort();
    found.dedup();
    Ok(found)
}

fn expand_parts(path: &Path, parts: &[&str], found: &mut Vec<PathBuf>) -> Result<(), String> {
    let Some((part, remaining)) = parts.split_first() else {
        if path.exists() {
            found.push(path.to_path_buf());
        }
        return Ok(());
    };
    if *part == "**" {
        expand_parts(path, remaining, found)?;
        for entry in read_entries(path)? {
            if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                expand_parts(&entry.path(), parts, found)?;
            }
        }
        return Ok(());
    }
    if !part.contains(['*', '?']) {
        return expand_parts(&path.join(part), remaining, found);
    }
    for entry in read_entries(path)? {
        if component_matches(part, &entry.file_name().to_string_lossy()) {
            expand_parts(&entry.path(), remaining, found)?;
        }
    }
    Ok(())
}

fn read_entries(path: &Path) -> Result<Vec<fs::DirEntry>, String> {
    let entries = match fs::read_dir(path) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(format!("{} unreadable ({error})", path.display())),
    };
    entries
        .map(|entry| entry.map_err(|error| format!("{} unreadable ({error})", path.display())))
        .collect()
}

fn component_matches(pattern: &str, name: &str) -> bool {
    let pattern = pattern.as_bytes();
    let name = name.as_bytes();
    let (mut p, mut n, mut star, mut retry) = (0, 0, None, 0);
    while n < name.len() {
        if p < pattern.len() && (pattern[p] == b'?' || pattern[p] == name[n]) {
            p += 1;
            n += 1;
        } else if p < pattern.len() && pattern[p] == b'*' {
            star = Some(p);
            p += 1;
            retry = n;
        } else if let Some(star_at) = star {
            p = star_at + 1;
            retry += 1;
            n = retry;
        } else {
            return false;
        }
    }
    while pattern.get(p) == Some(&b'*') {
        p += 1;
    }
    p == pattern.len()
}

#[cfg(test)]
mod tests {
    use super::component_matches;

    #[test]
    fn component_match_handles_stars_and_single_characters() {
        assert!(component_matches("crate-*", "crate-a"));
        assert!(component_matches("cr?te", "crate"));
        assert!(!component_matches("crate-*", "crate"));
    }
}
