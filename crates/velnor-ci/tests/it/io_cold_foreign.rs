// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::process::Command;

use crate::io::Repo;
use crate::io_cold::{helper, stale_target};

fn foreign_cache_link(operation: &str, tracked: bool) {
    let caller = Repo::new("cold-chain-caller");
    let owner = Repo::new("cold-chain-owner");
    let external = Repo::new("cold-chain-external");
    for repo in [&caller, &owner] {
        repo.write("src/lib.rs", "pub fn one() {}\n");
    }
    external.write_unstaged("artifacts/target/stale", "cached artifact\n");
    external.write_unstaged("artifacts/sibling-sentinel", "sibling artifact\n");
    let cache = external.root().join("artifacts");
    let link = owner.root().join("cache");
    std::os::unix::fs::symlink(&cache, &link).unwrap_or_else(|e| unreachable!("{e}"));
    std::os::unix::fs::symlink(&link, caller.root().join("alias"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    if tracked {
        owner.git(&["add", "--force", "cache"]);
    }
    let indices = [&caller, &owner].map(|repo| {
        std::fs::read(repo.root().join(".git/index")).unwrap_or_else(|e| unreachable!("{e}"))
    });
    let output = helper(
        &caller,
        &["inputs", operation, "--target-dir", "alias/target"],
    );
    if !tracked && operation == "restore" {
        assert!(!cache.join("target").exists(), "{output:?}");
    } else {
        assert_eq!(
            std::fs::read(cache.join("target/stale")).unwrap_or_default(),
            b"cached artifact\n",
            "{tracked} {operation}: {output:?}"
        );
    }
    assert_eq!(output.status.success(), !tracked, "{output:?}");
    assert_eq!(
        cache.join("target/.velnor/inputs.tsv").is_file(),
        !tracked && operation == "save"
    );
    assert_eq!(
        std::fs::read(cache.join("sibling-sentinel")).unwrap_or_default(),
        b"sibling artifact\n"
    );
    assert_eq!(std::fs::read_link(&link).ok(), Some(cache));
    assert_eq!(
        std::fs::read_link(caller.root().join("alias")).ok(),
        Some(link)
    );
    for (repo, index) in [&caller, &owner].into_iter().zip(indices) {
        assert_eq!(
            std::fs::read(repo.root().join(".git/index")).ok(),
            Some(index)
        );
        assert_eq!(
            std::fs::read(repo.root().join("src/lib.rs")).unwrap_or_default(),
            b"pub fn one() {}\n"
        );
    }
}

#[test]
fn missing_snapshot_restore_preserves_target_through_foreign_tracked_cache_link() {
    foreign_cache_link("restore", true);
}

#[test]
fn save_refuses_target_through_foreign_tracked_cache_link() {
    foreign_cache_link("save", true);
}

#[test]
fn missing_snapshot_restore_discards_target_through_foreign_untracked_cache_link() {
    foreign_cache_link("restore", false);
}

#[test]
fn save_allows_target_through_foreign_untracked_cache_link() {
    foreign_cache_link("save", false);
}

#[test]
fn a_corrupt_git_index_fails_restored_instead_of_a_partial_cold_build() {
    let repo = Repo::new("cold-corrupt-index");
    repo.write("src/lib.rs", "pub fn one() {}\n");
    repo.write("app/Cargo.toml", "[package]\n");
    stale_target(&repo, "target");
    stale_target(&repo, "app/target");
    std::fs::write(repo.root().join(".git/index"), b"not an index")
        .unwrap_or_else(|e| unreachable!("{e}"));
    let output = helper(&repo, &["restored"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{stdout}{stderr}");
    assert!(!stdout.contains("building cold"), "{stdout}");
    assert!(stderr.contains("tracked Cargo manifests"), "{stderr}");
}

#[test]
fn restored_without_git_on_the_path_fails_instead_of_a_partial_cold_build() {
    let repo = Repo::new("cold-no-git");
    repo.write("src/lib.rs", "pub fn one() {}\n");
    repo.write("rust/Cargo.toml", "[package]\n");
    stale_target(&repo, "rust/target");
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .arg("restored")
        .current_dir(repo.root())
        .env("PATH", "")
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!output.status.success(), "{stdout}");
    assert!(!stdout.contains("building cold"), "{stdout}");
    assert!(repo.root().join("rust/target").exists());
}
