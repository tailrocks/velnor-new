// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, UNIX_EPOCH};

use crate::io::Repo;

fn helper(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .args(args)
        .current_dir(root)
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"))
}

fn success(root: &Path, args: &[&str]) {
    let output = helper(root, args);
    assert!(output.status.success(), "{args:?}: {output:?}");
}

fn checkout(metadata: bool) -> Repo {
    let repo = Repo::new("restored-targets");
    repo.write_all(&[
        ("mise.toml", "[tools]\nrust='1.99.1'\n"),
        (
            ".gitignore",
            "**/target/\n/cache-a/\n/cache-b/\n/configured-a/\n/configured-b/\n",
        ),
        (
            "a/Cargo.toml",
            "[package]\nname='a'\nversion='0.1.0'\nedition='2024'\n",
        ),
        ("a/src/lib.rs", "pub fn a() {}\n"),
        (
            "b/Cargo.toml",
            "[package]\nname='b'\nversion='0.1.0'\nedition='2024'\n",
        ),
        ("b/src/lib.rs", "pub fn b() {}\n"),
    ]);
    if metadata {
        for name in ["a", "b"] {
            repo.write(
                &format!("{name}/.cargo/config.toml"),
                &format!("[build]\ntarget-dir='../configured-{name}'\n"),
            );
            let output = Command::new("cargo")
                .args(["generate-lockfile", "--offline"])
                .current_dir(repo.root().join(name))
                .env_remove("CARGO_TARGET_DIR")
                .output()
                .unwrap_or_else(|e| unreachable!("{e}"));
            assert!(output.status.success(), "{output:?}");
        }
        repo.git(&["add", "--all"]);
        velnor_gen::io::write_metadata(repo.root()).unwrap_or_else(|e| unreachable!("{e}"));
    }
    repo
}

fn restore(repo: &Repo, name: &str) {
    let root = repo.root().join(name);
    let target = if name == "b" {
        repo.root().join("cache-b").to_string_lossy().into_owned()
    } else {
        format!("../cache-{name}")
    };
    success(&root, &["inputs", "save", "--target-dir", &target]);
    success(&root, &["inputs", "restore", "--target-dir", &target]);
    repo.write_unstaged(&format!("cache-{name}/stale"), "cache\n");
}

fn fail_restored(repo: &Repo) -> Output {
    repo.set_mtime("a/src/lib.rs", UNIX_EPOCH - Duration::from_secs(1));
    helper(repo.root(), &["restored"])
}

fn receipt_dir(repo: &Repo) -> PathBuf {
    let output = Command::new("git")
        .args([
            "rev-parse",
            "--path-format=absolute",
            "--git-path",
            "velnor-restored-targets",
        ])
        .current_dir(repo.root())
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(output.status.success(), "{output:?}");
    PathBuf::from(
        String::from_utf8(output.stdout)
            .unwrap_or_else(|e| unreachable!("{e}"))
            .trim(),
    )
}

#[test]
fn restored_discards_explicit_targets_from_every_restore_process() {
    for metadata in [false, true] {
        let repo = checkout(metadata);
        std::thread::scope(|scope| {
            for name in ["a", "b"] {
                let repo = &repo;
                scope.spawn(move || restore(repo, name));
            }
        });
        let output = fail_restored(&repo);
        assert!(output.status.success(), "{output:?}");
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("building cold"),
            "{output:?}"
        );
        for name in ["a", "b"] {
            assert!(
                !repo.root().join(format!("cache-{name}")).exists(),
                "metadata={metadata}: {name} custom cache survived"
            );
            assert!(repo.root().join(format!("{name}/src/lib.rs")).is_file());
        }
    }
}

#[test]
fn successful_restored_does_not_reuse_consumed_targets() {
    let repo = checkout(false);
    restore(&repo, "a");
    success(repo.root(), &["restored"]);
    repo.write_unstaged("a/target/stale", "default\n");
    let output = fail_restored(&repo);
    assert!(output.status.success(), "{output:?}");
    assert!(repo.root().join("cache-a/stale").is_file());
    assert!(!repo.root().join("a/target").exists());
}

#[test]
fn restored_cold_fallback_replaces_a_custom_target_link_for_the_next_cargo_build() {
    let repo = checkout(false);
    let external = Repo::new("external-cold-target");
    external.write_unstaged("cache/stale", "external cache\n");
    let target = repo.root().join("custom-link");
    std::os::unix::fs::symlink(external.root().join("cache"), &target)
        .unwrap_or_else(|e| unreachable!("{e}"));
    for operation in ["save", "restore"] {
        success(
            &repo.root().join("a"),
            &["inputs", operation, "--target-dir", "../custom-link"],
        );
    }
    let output = fail_restored(&repo);
    assert!(output.status.success(), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("building cold"),
        "{output:?}"
    );
    assert!(
        fs::symlink_metadata(&target).is_err(),
        "custom target link survived cold fallback"
    );
    assert!(external.root().join("cache/stale").is_file());
    repo.set_mtime(
        "a/src/lib.rs",
        UNIX_EPOCH + Duration::from_secs(1_700_000_000),
    );
    let output = Command::new("cargo")
        .args([
            "build",
            "--offline",
            "--manifest-path",
            "a/Cargo.toml",
            "--target-dir",
            "custom-link",
        ])
        .current_dir(repo.root())
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTC_WRAPPER")
        .env_remove("RUSTC_WORKSPACE_WRAPPER")
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(output.status.success(), "{output:?}");
    assert!(target.join("debug/liba.rlib").is_file());
}

#[test]
fn invalid_pending_targets_prevent_all_cold_deletions() {
    for unsafe_target in [false, true] {
        let repo = checkout(false);
        restore(&repo, "a");
        let directory = receipt_dir(&repo);
        fs::create_dir_all(&directory).unwrap_or_else(|e| unreachable!("{e}"));
        let (name, bytes) = if unsafe_target {
            let target =
                fs::canonicalize(repo.root().join(".git")).unwrap_or_else(|e| unreachable!("{e}"));
            let text = target.to_str().unwrap_or_else(|| unreachable!());
            (
                format!("{}.json", velnor_rust::sha256_hex(text.as_bytes())),
                serde_json::to_vec(&target).unwrap_or_else(|e| unreachable!("{e}")),
            )
        } else {
            ("invalid.json".to_owned(), b"invalid\n".to_vec())
        };
        fs::write(directory.join(name), bytes).unwrap_or_else(|e| unreachable!("{e}"));
        repo.write_unstaged("a/target/stale", "default\n");
        let output = fail_restored(&repo);
        assert!(!output.status.success(), "{output:?}");
        assert!(!String::from_utf8_lossy(&output.stdout).contains("building cold"));
        assert!(repo.root().join("cache-a/stale").is_file());
        assert!(repo.root().join("a/target/stale").is_file());
        assert!(repo.root().join(".git").is_dir());
    }
}
