// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::{OrchestratorError, internal::internal};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TreeItem {
    pub name: String,
    pub mode: String,
    pub sha: String,
}

#[derive(Clone)]
pub(crate) struct Change {
    pub mode: String,
    pub sha: String,
}

pub(crate) trait TreeRead {
    fn read_tree(&self, sha: &str) -> Result<Vec<TreeItem>, OrchestratorError>;
}

pub(crate) struct GitHubTrees<'a> {
    pub api: &'a dyn crate::crate_release_github::GitHubAdmissionRead,
    pub repository: &'a str,
}

impl TreeRead for GitHubTrees<'_> {
    fn read_tree(&self, sha: &str) -> Result<Vec<TreeItem>, OrchestratorError> {
        let invalid = || internal("release_pr_tree_invalid");
        let Some((owner, name)) = self.repository.split_once('/') else {
            return Err(invalid());
        };
        if !velnor_actions_contract::config::is_valid_github_owner(owner)
            || !velnor_actions_contract::config::is_valid_github_repo_name(name)
            || sha.len() != 40
            || !sha
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(invalid());
        }
        let response = self
            .api
            .get(&format!("repos/{}/git/trees/{sha}", self.repository))?;
        if response["sha"].as_str() != Some(sha) || response["truncated"].as_bool() != Some(false) {
            return Err(invalid());
        }
        let values = response["tree"]
            .as_array()
            .filter(|value| value.len() <= 100_000)
            .ok_or_else(invalid)?;
        let mut entries = Vec::with_capacity(values.len());
        for value in values {
            let mode = value["mode"].as_str().ok_or_else(invalid)?;
            let kind = value["type"].as_str().ok_or_else(invalid)?;
            if !matches!(
                (mode, kind),
                ("040000", "tree")
                    | ("100644" | "100755" | "120000", "blob")
                    | ("160000", "commit")
            ) {
                return Err(invalid());
            }
            entries.push(TreeItem {
                name: value["path"].as_str().ok_or_else(invalid)?.to_owned(),
                mode: mode.to_owned(),
                sha: value["sha"].as_str().ok_or_else(invalid)?.to_owned(),
            });
        }
        if git_tree_sha(&entries)? != sha {
            return Err(invalid());
        }
        Ok(entries)
    }
}

pub(crate) fn regular_file_mode<R: TreeRead>(
    reader: &R,
    root: &str,
    path: &str,
) -> Result<String, OrchestratorError> {
    let invalid = || internal("release_pr_file_mode");
    let mut parent = root.to_owned();
    let mut parts = path.split('/').peekable();
    if path.len() > 1024 || path.contains('\\') || path.bytes().any(|b| b.is_ascii_control()) {
        return Err(invalid());
    }
    let mut depth = 0usize;
    while let Some(name) = parts.next() {
        depth += 1;
        if depth > 512 || name.is_empty() || matches!(name, "." | ".." | ".git") {
            return Err(invalid());
        }
        let entries = reader.read_tree(&parent)?;
        if git_tree_sha(&entries)? != parent {
            return Err(invalid());
        }
        let entry = entries.iter().find(|entry| entry.name == name);
        if parts.peek().is_none() {
            return match entry {
                None => Ok("100644".to_owned()),
                Some(entry) if matches!(entry.mode.as_str(), "100644" | "100755") => {
                    Ok(entry.mode.clone())
                }
                Some(_) => Err(invalid()),
            };
        }
        let entry = entry
            .filter(|entry| entry.mode == "040000")
            .ok_or_else(invalid)?;
        parent.clone_from(&entry.sha);
    }
    Err(invalid())
}

pub(crate) fn overlay_tree<R: TreeRead>(
    reader: &R,
    root: &str,
    changes: &std::collections::BTreeMap<String, Change>,
) -> Result<String, OrchestratorError> {
    let invalid = || internal("release_pr_overlay_invalid");
    if changes.is_empty() || changes.len() > 256 {
        return Err(invalid());
    }
    for (path, change) in changes {
        if path.len() > 1024
            || path.contains('\\')
            || path.bytes().any(|byte| byte.is_ascii_control())
            || path
                .split('/')
                .any(|part| part.is_empty() || matches!(part, "." | ".." | ".git"))
            || !matches!(change.mode.as_str(), "100644" | "100755")
        {
            return Err(invalid());
        }
        git_tree_sha(&[TreeItem {
            name: "validation".into(),
            mode: change.mode.clone(),
            sha: change.sha.clone(),
        }])?;
    }
    let references: Vec<_> = changes
        .iter()
        .map(|(path, change)| (path.as_str(), change))
        .collect();
    overlay_directory(reader, root, &references, 0)
}

fn overlay_directory<R: TreeRead>(
    reader: &R,
    tree_sha: &str,
    changes: &[(&str, &Change)],
    depth: usize,
) -> Result<String, OrchestratorError> {
    let invalid = || internal("release_pr_overlay_invalid");
    if depth > 512 {
        return Err(invalid());
    }
    let entries = reader.read_tree(tree_sha)?;
    if git_tree_sha(&entries)? != tree_sha {
        return Err(invalid());
    }
    let mut by_name: std::collections::BTreeMap<_, _> = entries
        .into_iter()
        .map(|item| (item.name.clone(), item))
        .collect();
    let mut grouped: std::collections::BTreeMap<&str, Vec<(&str, &Change)>> =
        std::collections::BTreeMap::new();
    for (path, change) in changes {
        let (name, rest) = path
            .split_once('/')
            .map_or((*path, ""), |(name, rest)| (name, rest));
        grouped.entry(name).or_default().push((rest, change));
    }
    for (name, group) in grouped {
        if group.len() == 1 && group[0].0.is_empty() {
            let change = group[0].1;
            if let Some(old) = by_name.get(name) {
                if old.mode != change.mode || old.sha == change.sha {
                    return Err(invalid());
                }
            } else if change.mode != "100644" {
                return Err(invalid());
            }
            by_name.insert(
                name.to_owned(),
                TreeItem {
                    name: name.to_owned(),
                    mode: change.mode.clone(),
                    sha: change.sha.clone(),
                },
            );
            continue;
        }
        if group.iter().any(|(rest, _)| rest.is_empty()) {
            return Err(invalid());
        }
        let old = by_name.get(name).ok_or_else(invalid)?;
        if old.mode != "040000" {
            return Err(invalid());
        }
        let sha = overlay_directory(reader, &old.sha, &group, depth + 1)?;
        by_name.insert(
            name.to_owned(),
            TreeItem {
                name: name.to_owned(),
                mode: "040000".into(),
                sha,
            },
        );
    }
    git_tree_sha(&by_name.into_values().collect::<Vec<_>>())
}

pub(crate) fn git_tree_sha(entries: &[TreeItem]) -> Result<String, OrchestratorError> {
    use sha1_checked::{Digest as _, Sha1};
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let invalid = || internal("release_pr_tree_invalid");
    if entries.len() > 100_000 {
        return Err(invalid());
    }
    let mut seen = std::collections::BTreeSet::new();
    let mut rows = Vec::with_capacity(entries.len());
    for entry in entries {
        if entry.name.is_empty()
            || matches!(entry.name.as_str(), "." | "..")
            || entry.name.contains(['/', '\0'])
            || !seen.insert(entry.name.as_str())
            || !matches!(
                entry.mode.as_str(),
                "040000" | "100644" | "100755" | "120000" | "160000"
            )
            || entry.sha.len() != 40
        {
            return Err(invalid());
        }
        let mut id = Vec::with_capacity(20);
        for pair in entry.sha.as_bytes().as_chunks::<2>().0 {
            let digit = |byte| match byte {
                b'0'..=b'9' => Some(byte - b'0'),
                b'a'..=b'f' => Some(byte - b'a' + 10),
                _ => None,
            };
            id.push(digit(pair[0]).ok_or_else(invalid)? * 16 + digit(pair[1]).ok_or_else(invalid)?);
        }
        let mut key = entry.name.as_bytes().to_vec();
        key.push(if entry.mode == "040000" { b'/' } else { 0 });
        let mut row = if entry.mode == "040000" {
            b"40000 ".to_vec()
        } else {
            format!("{} ", entry.mode).into_bytes()
        };
        row.extend_from_slice(entry.name.as_bytes());
        row.push(0);
        row.extend_from_slice(&id);
        rows.push((key, row));
    }
    rows.sort_unstable_by(|left, right| left.0.cmp(&right.0));
    let len = rows
        .iter()
        .try_fold(0usize, |sum, (_, row)| sum.checked_add(row.len()))
        .ok_or_else(invalid)?;
    if len > 16 * 1024 * 1024 {
        return Err(invalid());
    }
    let mut hasher = Sha1::new();
    hasher.update(format!("tree {len}\0").as_bytes());
    for (_, row) in rows {
        hasher.update(&row);
    }
    let result = hasher.try_finalize();
    if result.has_collision() {
        return Err(invalid());
    }
    let mut digest = String::with_capacity(40);
    for byte in result.hash() {
        digest.push(char::from(HEX[usize::from(byte >> 4)]));
        digest.push(char::from(HEX[usize::from(byte & 15)]));
    }
    Ok(digest)
}

#[cfg(test)]
#[path = "crate_release_pr_tree_tests.rs"]
mod tests;
