// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use crate::io::Repo;
use crate::io_scope_cache::prepared;

fn run(root: &Path, operation: &str, target: &str, cache: &Path) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_velnor-ci"));
    command
        .current_dir(root)
        .env("GITHUB_JOB", "job")
        .args(["inputs", operation]);
    if operation == "restore-all" {
        command.args(["--root-target", ".", target]);
    } else {
        command.args(["--target-dir", target]);
    }
    command
        .arg("--scope-cache")
        .arg(cache)
        .output()
        .unwrap_or_else(|error| unreachable!("{error}"))
}

fn protected(root: &Path, operation: &str, cache: &Path) {
    let sentinel = root.join("custom-target/sentinel");
    let snapshot = root.join("custom-target/.velnor/inputs.tsv");
    fs::create_dir_all(snapshot.parent().unwrap_or_else(|| unreachable!()))
        .unwrap_or_else(|error| unreachable!("{error}"));
    fs::write(&sentinel, "artifact\n").unwrap_or_else(|error| unreachable!("{error}"));
    fs::write(&snapshot, "old snapshot\n").unwrap_or_else(|error| unreachable!("{error}"));
    let watched = [
        cache.to_path_buf(),
        root.join("src/lib.rs"),
        snapshot,
        sentinel,
    ];
    let before: Vec<_> = watched
        .iter()
        .map(|path| fs::read(path).unwrap_or_else(|error| unreachable!("{error}")))
        .collect();
    let output = run(root, operation, "custom-target", cache);
    assert!(
        !output.status.success(),
        "{operation}: {}: {output:?}",
        cache.display()
    );
    for (path, expected) in watched.iter().zip(before) {
        assert_eq!(
            fs::read(path).unwrap_or_default(),
            expected,
            "{operation}: {}: {output:?}",
            path.display()
        );
    }
}

#[test]
fn scope_cache_operations_refuse_tracked_files_git_metadata_and_symlink_aliases() {
    for operation in ["restore-all", "save"] {
        for kind in ["source", "git", "link", "source-parent", "git-parent"] {
            let (repo, _) = prepared();
            let outside = Repo::empty("protected-cache-alias");
            let cache = match kind {
                "source" => repo.root().join("src/lib.rs"),
                "git" => repo.root().join(".git/config"),
                "link" => {
                    let path = outside.root().join("scopes.json");
                    std::os::unix::fs::symlink(repo.root().join("src/lib.rs"), &path)
                        .unwrap_or_else(|error| unreachable!("{error}"));
                    path
                }
                _ => {
                    let parent = outside.root().join("parent");
                    let (directory, file) = if kind == "source-parent" {
                        ("src", "lib.rs")
                    } else {
                        (".git", "config")
                    };
                    std::os::unix::fs::symlink(repo.root().join(directory), &parent)
                        .unwrap_or_else(|error| unreachable!("{error}"));
                    parent.join(file)
                }
            };
            protected(repo.root(), operation, &cache);
        }
    }
}

#[test]
fn scope_cache_operations_refuse_linked_worktree_administrative_files() {
    for operation in ["restore-all", "save"] {
        let (repo, _) = prepared();
        let linked = Repo::empty("scope-cache-linked");
        let linked_text = linked.root().to_str().unwrap_or_else(|| unreachable!());
        repo.git(&[
            "worktree",
            "add",
            "--quiet",
            "--detach",
            linked_text,
            "HEAD",
        ]);
        let output = Command::new("git")
            .arg("-C")
            .arg(linked.root())
            .args(["rev-parse", "--path-format=absolute", "--git-path", "HEAD"])
            .output()
            .unwrap_or_else(|error| unreachable!("{error}"));
        assert!(output.status.success(), "{output:?}");
        let cache = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
        protected(linked.root(), operation, &cache);
    }
}

#[test]
fn unsafe_targets_are_refused_before_discarding_a_valid_scope_cache() {
    let (repo, cache) = prepared();
    repo.write_unstaged("target/keep", "tracked\n");
    repo.git(&["add", "--force", "target/keep"]);
    let before = fs::read(&cache).unwrap_or_else(|error| unreachable!("{error}"));
    let output = run(repo.root(), "restore-all", "target", &cache);
    assert!(!output.status.success(), "{output:?}");
    assert_eq!(fs::read(&cache).unwrap_or_default(), before);
    assert_eq!(
        fs::read(repo.root().join("target/keep")).unwrap_or_default(),
        b"tracked\n"
    );
}

#[test]
fn an_external_scope_cache_can_be_refreshed_and_stale_snapshots_are_invalidated() {
    let (repo, _) = prepared();
    let outside = Repo::empty("external-safe-cache");
    let cache = outside.root().join("scopes.json");
    let restored = run(repo.root(), "restore-all", "custom-target", &cache);
    assert!(restored.status.success(), "{restored:?}");
    let saved = run(repo.root(), "save", "custom-target", &cache);
    assert!(saved.status.success(), "{saved:?}");
    assert!(
        repo.root()
            .join("custom-target/.velnor/inputs.tsv")
            .is_file()
    );
    fs::write(&cache, "{").unwrap_or_else(|error| unreachable!("{error}"));
    repo.write_unstaged("custom-target/sentinel", "artifact\n");
    let rejected = run(repo.root(), "save", "custom-target", &cache);
    assert!(!rejected.status.success(), "{rejected:?}");
    assert!(
        !repo
            .root()
            .join("custom-target/.velnor/inputs.tsv")
            .exists()
    );
    assert_eq!(
        fs::read(repo.root().join("custom-target/sentinel")).unwrap_or_default(),
        b"artifact\n"
    );
}
