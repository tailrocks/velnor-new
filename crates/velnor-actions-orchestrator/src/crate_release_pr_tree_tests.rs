// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

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
        let output = git(self.0, &["ls-tree", "-z", sha]).map_err(|_| internal("fixture_git"))?;
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
#[expect(
    clippy::too_many_lines,
    reason = "The complete Git fixture and its boundaries are reviewed together"
)]
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
    assert!(super::regular_file_mode(&GitTrees(root.path()), &base, "alias/Cargo.toml").is_err());
    assert!(
        super::regular_file_mode(&GitTrees(root.path()), &base, "submodule/Cargo.toml").is_err()
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
