// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::{OrchestratorError, internal::internal};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct TreeItem {
    pub name: String,
    pub mode: String,
    pub sha: String,
}

#[derive(Clone)]
pub(super) struct Change {
    pub mode: String,
    pub sha: String,
}

pub(super) trait TreeRead {
    fn read_tree(&self, sha: &str) -> Result<Vec<TreeItem>, OrchestratorError>;
}

pub(super) struct GitHubTrees<'a> {
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

pub(super) fn regular_file_mode<R: TreeRead>(
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

pub(super) fn overlay_tree<R: TreeRead>(
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

pub(super) fn git_tree_sha(entries: &[TreeItem]) -> Result<String, OrchestratorError> {
    use sha1_checked::{Digest as _, Sha1};
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
        for pair in entry.sha.as_bytes().chunks_exact(2) {
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
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut digest = String::with_capacity(40);
    for byte in result.hash() {
        digest.push(char::from(HEX[usize::from(byte >> 4)]));
        digest.push(char::from(HEX[usize::from(byte & 15)]));
    }
    Ok(digest)
}

#[cfg(test)]
mod tests {
    use super::{Change, TreeItem, TreeRead, git_tree_sha, overlay_tree};
    use crate::{OrchestratorError, internal::internal};
    use std::collections::BTreeMap;
    use std::{fs, path::Path, process::Command};

    fn git(root: &Path, args: &[&str]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let output = Command::new("git")
            .current_dir(root)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .args(args)
            .output()?;
        if !output.status.success() {
            return Err("git fixture failed".into());
        }
        Ok(output.stdout)
    }

    fn entries(bytes: &[u8]) -> Result<Vec<TreeItem>, Box<dyn std::error::Error>> {
        bytes
            .split(|byte| *byte == 0)
            .filter(|line| !line.is_empty())
            .map(|line| {
                let text = std::str::from_utf8(line)?;
                let (metadata, name) = text.split_once('\t').ok_or("name")?;
                let mut parts = metadata.split(' ');
                let mode = parts.next().ok_or("mode")?.to_owned();
                let _kind = parts.next().ok_or("kind")?;
                let sha = parts.next().ok_or("sha")?.to_owned();
                Ok(TreeItem {
                    name: name.to_owned(),
                    mode,
                    sha,
                })
            })
            .collect()
    }

    #[test]
    fn computes_real_git_tree_identity_for_nested_and_prefix_names()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        git(root.path(), &["init", "-q"])?;
        fs::create_dir(root.path().join("alpha"))?;
        fs::write(root.path().join("alpha/file"), b"nested")?;
        fs::write(root.path().join("alpha.foo"), b"prefix")?;
        fs::write(root.path().join("zeta"), b"executable")?;
        fs::write(root.path().join("éclair"), b"unicode")?;
        git(root.path(), &["add", "."])?;
        git(root.path(), &["update-index", "--chmod=+x", "zeta"])?;
        let sha = String::from_utf8(git(root.path(), &["write-tree"])?)?
            .trim()
            .to_owned();
        let tree = entries(&git(root.path(), &["ls-tree", "-z", &sha])?)?;
        assert_eq!(git_tree_sha(&tree)?, sha);
        let mut reordered = tree.clone();
        reordered.reverse();
        assert_eq!(git_tree_sha(&reordered)?, sha);
        let mut duplicate = tree.clone();
        duplicate.push(tree[0].clone());
        assert!(git_tree_sha(&duplicate).is_err());
        let mut invalid = tree.clone();
        invalid[0].name = "../escape".into();
        assert!(git_tree_sha(&invalid).is_err());
        Ok(())
    }

    struct GitTrees<'a>(&'a Path);

    impl TreeRead for GitTrees<'_> {
        fn read_tree(&self, sha: &str) -> Result<Vec<TreeItem>, OrchestratorError> {
            let output =
                git(self.0, &["ls-tree", "-z", sha]).map_err(|_| internal("fixture_git"))?;
            entries(&output).map_err(|_| internal("fixture_git"))
        }
    }

    struct ForgedTrees<'a>(&'a Path);

    impl TreeRead for ForgedTrees<'_> {
        fn read_tree(&self, sha: &str) -> Result<Vec<TreeItem>, OrchestratorError> {
            let mut entries = GitTrees(self.0).read_tree(sha)?;
            if let Some(entry) = entries.first_mut() {
                entry.sha = "f".repeat(40);
            }
            Ok(entries)
        }
    }

    #[cfg(unix)]
    #[test]
    fn supports_deep_reviewed_workspace_paths() -> Result<(), OrchestratorError> {
        struct Trees(BTreeMap<String, Vec<TreeItem>>);
        impl TreeRead for Trees {
            fn read_tree(&self, sha: &str) -> Result<Vec<TreeItem>, OrchestratorError> {
                self.0
                    .get(sha)
                    .cloned()
                    .ok_or_else(|| internal("missing_tree"))
            }
        }
        let old_blob = crate::release_policy_admission::git_blob_sha1(b"old")?;
        let new_blob = crate::release_policy_admission::git_blob_sha1(b"new")?;
        let mut trees = BTreeMap::new();
        let leaf = vec![TreeItem {
            name: "Cargo.toml".into(),
            mode: "100644".into(),
            sha: old_blob,
        }];
        let mut root = git_tree_sha(&leaf)?;
        trees.insert(root.clone(), leaf);
        for _ in 0..40 {
            let dir = vec![TreeItem {
                name: "d".into(),
                mode: "040000".into(),
                sha: root,
            }];
            root = git_tree_sha(&dir)?;
            trees.insert(root.clone(), dir);
        }
        let path = format!("{}Cargo.toml", "d/".repeat(40));
        let reader = Trees(trees);
        assert_eq!(super::regular_file_mode(&reader, &root, &path)?, "100644");
        assert_ne!(
            overlay_tree(
                &reader,
                &root,
                &BTreeMap::from([(
                    path,
                    Change {
                        mode: "100644".into(),
                        sha: new_blob
                    }
                )])
            )?,
            root
        );
        Ok(())
    }

    #[test]
    fn overlays_real_git_tree_without_changing_unrelated_entries()
    -> Result<(), Box<dyn std::error::Error>> {
        use std::os::unix::fs::symlink;
        let root = tempfile::tempdir()?;
        git(root.path(), &["init", "-q"])?;
        fs::create_dir(root.path().join("crate"))?;
        fs::write(root.path().join("crate/Cargo.toml"), b"original")?;
        fs::write(root.path().join("crate.foo"), b"sibling")?;
        symlink("crate", root.path().join("alias"))?;
        git(root.path(), &["add", "."])?;
        git(
            root.path(),
            &[
                "update-index",
                "--add",
                "--cacheinfo",
                "160000,aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa,submodule",
            ],
        )?;
        let base = String::from_utf8(git(root.path(), &["write-tree"])?)?
            .trim()
            .to_owned();
        fs::write(root.path().join("crate/Cargo.toml"), b"updated")?;
        fs::write(root.path().join("crate/CHANGELOG.md"), b"changes")?;
        git(root.path(), &["add", "."])?;
        git(
            root.path(),
            &[
                "update-index",
                "--add",
                "--cacheinfo",
                "160000,aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa,submodule",
            ],
        )?;
        let expected = String::from_utf8(git(root.path(), &["write-tree"])?)?
            .trim()
            .to_owned();
        let cargo = crate::release_policy_admission::git_blob_sha1(b"updated")?;
        let changelog = crate::release_policy_admission::git_blob_sha1(b"changes")?;
        let changes = BTreeMap::from([
            (
                "crate/Cargo.toml".into(),
                Change {
                    mode: "100644".into(),
                    sha: cargo.clone(),
                },
            ),
            (
                "crate/CHANGELOG.md".into(),
                Change {
                    mode: "100644".into(),
                    sha: changelog,
                },
            ),
        ]);
        assert_eq!(
            overlay_tree(&GitTrees(root.path()), &base, &changes)?,
            expected
        );
        assert_eq!(
            super::regular_file_mode(&GitTrees(root.path()), &base, "crate/Cargo.toml")?,
            "100644"
        );
        assert_eq!(
            super::regular_file_mode(&GitTrees(root.path()), &base, "crate/CHANGELOG.md")?,
            "100644"
        );
        assert!(
            super::regular_file_mode(&GitTrees(root.path()), &base, "alias/Cargo.toml").is_err()
        );
        assert!(
            super::regular_file_mode(&GitTrees(root.path()), &base, "submodule/Cargo.toml")
                .is_err()
        );
        assert!(
            super::regular_file_mode(&ForgedTrees(root.path()), &base, "crate/Cargo.toml").is_err()
        );
        assert!(overlay_tree(&ForgedTrees(root.path()), &base, &changes).is_err());
        let symlink_path = BTreeMap::from([(
            "alias/Cargo.toml".into(),
            Change {
                mode: "100644".into(),
                sha: cargo.clone(),
            },
        )]);
        assert!(overlay_tree(&GitTrees(root.path()), &base, &symlink_path).is_err());
        let gitlink_path = BTreeMap::from([(
            "submodule/Cargo.toml".into(),
            Change {
                mode: "100644".into(),
                sha: cargo.clone(),
            },
        )]);
        assert!(overlay_tree(&GitTrees(root.path()), &base, &gitlink_path).is_err());
        let mode_change = BTreeMap::from([(
            "crate/Cargo.toml".into(),
            Change {
                mode: "100755".into(),
                sha: cargo,
            },
        )]);
        assert!(overlay_tree(&GitTrees(root.path()), &base, &mode_change).is_err());
        let same = crate::release_policy_admission::git_blob_sha1(b"original")?;
        let unchanged = BTreeMap::from([(
            "crate/Cargo.toml".into(),
            Change {
                mode: "100644".into(),
                sha: same,
            },
        )]);
        assert!(overlay_tree(&GitTrees(root.path()), &base, &unchanged).is_err());
        Ok(())
    }
}
