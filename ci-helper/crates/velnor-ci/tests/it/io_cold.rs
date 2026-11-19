// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::process::{Command, Output};
use std::time::{Duration, UNIX_EPOCH};

use velnor_ci::io::{break_time_ties_with, cold_on_failure, every_root_target};

use crate::io::Repo;

fn helper(repo: &Repo, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .args(args)
        .current_dir(repo.root())
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"))
}

fn stale_target(repo: &Repo, relative: &str) {
    repo.write_unstaged(
        &format!("{relative}/debug/.fingerprint/stale/lib-stale"),
        "x",
    );
}

#[test]
fn a_malformed_saved_file_makes_restore_a_cold_build_with_no_stale_target() {
    let repo = Repo::new("cold-malformed");
    repo.write("src/lib.rs", "pub fn one() {}\n");
    stale_target(&repo, "target");
    repo.write_unstaged("target/.velnor/inputs.tsv", "not a valid line\n");
    let output = helper(&repo, &["inputs", "restore"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{stdout}");
    assert!(stdout.contains("building cold"), "{stdout}");
    assert!(stdout.contains("inputs.tsv"), "{stdout}");
    assert!(!repo.root().join("target").exists());
    assert!(repo.root().join("src/lib.rs").is_file());
}

#[test]
fn a_restore_with_bad_arguments_still_fails() {
    let repo = Repo::new("cold-args");
    let output = helper(&repo, &["inputs", "restore", "--bogus"]);
    assert!(!output.status.success());
}

#[test]
fn rounding_detected_after_tie_breaking_makes_a_cold_build_in_every_root() {
    let repo = Repo::new("cold-rounding");
    for path in ["build.rs", "src/lib.rs"] {
        repo.write(path, "x\n");
    }
    repo.write("app/Cargo.toml", "[package]\n");
    let tied = UNIX_EPOCH + Duration::new(1_700_000_000, 193_657_627);
    for path in [".gitignore", "build.rs", "src/lib.rs"] {
        repo.set_mtime(path, tied);
    }
    stale_target(&repo, "target");
    stale_target(&repo, "app/target");
    let report = cold_on_failure(
        &every_root_target(repo.root()).unwrap_or_else(|e| unreachable!("{e}")),
        "restored",
        || break_time_ties_with(repo.root(), &|_, _| Ok(())),
    )
    .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(
        report.contains("still share a modification time"),
        "{report}"
    );
    assert!(report.contains("building cold"), "{report}");
    assert!(!repo.root().join("target").exists());
    assert!(!repo.root().join("app/target").exists());
}

#[test]
fn a_non_utf8_tracked_path_outside_target_allows_verified_cold_deletion() {
    let repo = Repo::new("cold-non-utf8");
    repo.write("src/lib.rs", "pub fn one() {}\n");
    let blob = Command::new("git")
        .arg("-C")
        .arg(repo.root())
        .args(["hash-object", "-w", "--stdin"])
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    let blob = String::from_utf8_lossy(&blob.stdout).trim().to_owned();
    let entry = format!("100644,{blob},");
    let mut path = entry.into_bytes();
    path.extend_from_slice(b"src/caf\xe9.rs");
    let status = Command::new("git")
        .arg("-C")
        .arg(repo.root())
        .args(["update-index", "--add", "--cacheinfo"])
        .arg(OsStr::from_bytes(&path))
        .status()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(status.success());
    stale_target(&repo, "target");
    let output = helper(&repo, &["restored"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{output:?}");
    assert!(stdout.contains("not valid UTF-8"), "{stdout}");
    assert!(stdout.contains("building cold"), "{stdout}");
    assert!(!repo.root().join("target").exists());
    assert_eq!(
        std::fs::read(repo.root().join("src/lib.rs")).unwrap_or_default(),
        b"pub fn one() {}\n"
    );
    repo.git(&["ls-files", "--error-unmatch", "src/lib.rs"]);
}

fn reject_target_under_tracked_cache_link(destination: &str, spelling: &str, operation: &str) {
    let repo = Repo::new("cold-tracked-cache-link");
    let external = (destination == "external").then(|| Repo::new("cold-external-cache"));
    repo.write("src/lib.rs", "pub fn one() {}\n");
    let source_time = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    repo.set_mtime("src/lib.rs", source_time);
    let cache = external.as_ref().map_or_else(
        || repo.root().join("cache-real"),
        |repo| repo.root().join("cache"),
    );
    std::fs::create_dir_all(cache.join("target")).unwrap_or_else(|e| unreachable!("{e}"));
    std::fs::write(cache.join("target/stale"), b"cached artifact\n")
        .unwrap_or_else(|e| unreachable!("{e}"));
    std::fs::write(cache.join("sibling-sentinel"), b"sibling artifact\n")
        .unwrap_or_else(|e| unreachable!("{e}"));
    std::os::unix::fs::symlink(&cache, repo.root().join("cache"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    repo.git(&["add", "--force", "cache"]);
    if spelling.starts_with("alias/") {
        std::os::unix::fs::symlink("cache", repo.root().join("alias"))
            .unwrap_or_else(|e| unreachable!("{e}"));
    }
    let index =
        std::fs::read(repo.root().join(".git/index")).unwrap_or_else(|e| unreachable!("{e}"));
    let source =
        std::fs::read(repo.root().join("src/lib.rs")).unwrap_or_else(|e| unreachable!("{e}"));
    let output = helper(&repo, &["inputs", operation, "--target-dir", spelling]);
    assert_eq!(
        std::fs::read(cache.join("target/stale")).unwrap_or_default(),
        b"cached artifact\n",
        "{destination} {spelling} {operation}"
    );
    assert_eq!(
        std::fs::read(cache.join("sibling-sentinel")).unwrap_or_default(),
        b"sibling artifact\n",
        "{destination} {spelling} {operation}"
    );
    assert!(
        !output.status.success(),
        "{destination} {spelling} {operation}: {output:?}"
    );
    assert_eq!(
        std::fs::read_link(repo.root().join("cache")).ok(),
        Some(cache.clone())
    );
    assert_eq!(
        std::fs::read_link(repo.root().join("alias")).ok(),
        spelling
            .starts_with("alias/")
            .then(|| std::path::PathBuf::from("cache"))
    );
    assert_eq!(
        std::fs::read(repo.root().join(".git/index")).ok(),
        Some(index)
    );
    assert_eq!(
        std::fs::read(repo.root().join("src/lib.rs")).ok(),
        Some(source)
    );
    assert_eq!(repo.mtime("src/lib.rs"), source_time);
    assert!(!cache.join("target/.velnor/inputs.tsv").exists());
}

#[test]
fn missing_snapshot_restore_rejects_external_tracked_cache_target() {
    reject_target_under_tracked_cache_link("external", "cache/target", "restore");
}

#[test]
fn missing_snapshot_restore_rejects_internal_tracked_cache_target() {
    reject_target_under_tracked_cache_link("internal", "cache/target", "restore");
}

#[test]
fn missing_snapshot_restore_rejects_external_target_through_untracked_alias() {
    reject_target_under_tracked_cache_link("external", "alias/target", "restore");
}

#[test]
fn missing_snapshot_restore_rejects_internal_target_through_untracked_alias() {
    reject_target_under_tracked_cache_link("internal", "alias/target", "restore");
}

#[test]
fn save_rejects_external_tracked_cache_target() {
    reject_target_under_tracked_cache_link("external", "cache/target", "save");
}

#[test]
fn save_rejects_internal_tracked_cache_target() {
    reject_target_under_tracked_cache_link("internal", "cache/target", "save");
}

#[test]
fn save_rejects_external_target_through_untracked_alias() {
    reject_target_under_tracked_cache_link("external", "alias/target", "save");
}

#[test]
fn save_rejects_internal_target_through_untracked_alias() {
    reject_target_under_tracked_cache_link("internal", "alias/target", "save");
}

#[test]
fn a_missing_snapshot_removes_the_target_and_preserves_every_other_file() {
    for target in ["target", "cache/target"] {
        let local = Repo::new("cold-local-cache");
        local.write("src/lib.rs", "pub fn one() {}\n");
        let source_time = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        local.set_mtime("src/lib.rs", source_time);
        local.write_unstaged(&format!("{target}/stale"), "cached artifact\n");
        local.write_unstaged("cache/sibling-sentinel", "sibling artifact\n");
        let output = helper(&local, &["inputs", "restore", "--target-dir", target]);
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(output.status.success(), "{output:?}");
        assert!(!local.root().join(target).exists(), "{stdout}");
        assert!(stdout.contains("building cold"), "{stdout}");
        assert_eq!(
            std::fs::read(local.root().join("src/lib.rs")).unwrap_or_default(),
            b"pub fn one() {}\n"
        );
        assert_eq!(local.mtime("src/lib.rs"), source_time);
        assert_eq!(
            std::fs::read(local.root().join("cache/sibling-sentinel")).unwrap_or_default(),
            b"sibling artifact\n"
        );
        assert!(
            !local
                .root()
                .join(target)
                .join(".velnor/inputs.tsv")
                .exists()
        );
    }
}

#[test]
fn a_mismatched_snapshot_removes_only_its_target() {
    let repo = Repo::new("cold-mismatched");
    repo.write(".gitignore", "/target/\n/cache/\n");
    repo.write_all(&[("data/a.txt", "a\n"), ("data/b.txt", "b\n")]);
    repo.link("data/a.txt", "data/link.txt");
    let saved = helper(&repo, &["inputs", "save", "--target-dir", "cache/target"]);
    assert!(saved.status.success(), "{saved:?}");
    repo.write_unstaged("cache/target/stale", "cached artifact\n");
    repo.write_unstaged("cache/sibling-sentinel", "sibling artifact\n");
    std::fs::remove_file(repo.root().join("data/link.txt")).unwrap_or_else(|e| unreachable!("{e}"));
    repo.link("data/b.txt", "data/link.txt");
    let output = helper(
        &repo,
        &["inputs", "restore", "--target-dir", "cache/target"],
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{output:?}");
    assert!(!repo.root().join("cache/target").exists());
    assert!(
        stdout.contains("tracked symlinks are new, changed or deleted"),
        "{stdout}"
    );
    assert!(stdout.contains("building cold"), "{stdout}");
    assert_eq!(
        std::fs::read_link(repo.root().join("data/link.txt")).ok(),
        Some("b.txt".into())
    );
    assert_eq!(
        std::fs::read(repo.root().join("data/b.txt")).unwrap_or_default(),
        b"b\n"
    );
    assert_eq!(
        std::fs::read(repo.root().join("cache/sibling-sentinel")).unwrap_or_default(),
        b"sibling artifact\n"
    );
}

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
