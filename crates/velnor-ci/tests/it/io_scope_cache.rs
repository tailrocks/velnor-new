// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, UNIX_EPOCH};

use crate::io::Repo;
use crate::io_inputs_batch::fixture;

fn run(repo: &Repo, root: &str, job: &str, operation: &str, cache: &Path) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_velnor-ci"));
    velnor_test_support::trust_mise_dir(&mut command, repo.root());
    command
        .current_dir(repo.root().join(root))
        .env("GITHUB_JOB", job)
        .args(["inputs", operation]);
    if operation == "restore-all" {
        command.args(["--root-target", ".", "custom-target"]);
    } else {
        command.args(["--target-dir", "custom-target"]);
    }
    command
        .arg("--scope-cache")
        .arg(cache)
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"))
}

pub(crate) fn prepared() -> (Repo, PathBuf) {
    let repo = fixture();
    repo.write("data.bin", "before\n");
    repo.write(".velnor/rust-inputs.lock", "data.bin\tone\n");
    std::os::unix::fs::symlink("../data.bin", repo.root().join("src/data-link"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    repo.git(&["add", "src/data-link"]);
    repo.commit();
    let cache = repo.root().join("job-temp/scopes.json");
    let output = run(&repo, "", "job", "restore-all", &cache);
    assert!(output.status.success(), "{output:?}");
    let output = run(&repo, "", "job", "save", &cache);
    assert!(output.status.success(), "{output:?}");
    assert!(
        repo.root()
            .join("custom-target/.velnor/inputs.tsv")
            .is_file()
    );
    (repo, cache)
}

fn rejected(repo: &Repo, cache: &Path, root: &str, job: &str) -> Output {
    let output = run(repo, root, job, "save", cache);
    assert!(!output.status.success(), "{output:?}");
    assert!(
        !repo
            .root()
            .join(root)
            .join("custom-target")
            .join(velnor_ci::inputs::inputs_file(if root.is_empty() {
                ""
            } else {
                "one/"
            }))
            .exists()
    );
    output
}

#[test]
fn malformed_or_missing_scope_caches_refuse_save_and_remove_the_old_snapshot() {
    let (repo, cache) = prepared();
    let pristine = pristine_copy(&repo);
    for damage in ["missing", "truncated", "schema", "digest", "body"] {
        reset_to(&repo, &pristine);
        let mut envelope: serde_json::Value =
            serde_json::from_slice(&fs::read(&cache).unwrap_or_default())
                .unwrap_or_else(|e| unreachable!("{e}"));
        match damage {
            "missing" => fs::remove_file(&cache).unwrap_or_else(|e| unreachable!("{e}")),
            "truncated" => {
                fs::write(&cache, "{").unwrap_or_else(|e| unreachable!("{e}"));
            }
            _ => {
                match damage {
                    "schema" => envelope["schema"] = 2.into(),
                    "digest" => envelope["digest"] = "bad".into(),
                    _ => {
                        envelope["body"] = "{}".into();
                        envelope["digest"] = velnor_rust::sha256_hex(b"{}").into();
                    }
                }
                fs::write(&cache, serde_json::to_vec(&envelope).unwrap_or_default())
                    .unwrap_or_else(|e| unreachable!("{e}"));
            }
        }
        rejected(&repo, &cache, "", "job");
    }
}

fn copy_into(from: &Path, to: &Path) {
    let output = Command::new("cp")
        .arg("-pR")
        .arg(from.join("."))
        .arg(to.join("."))
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(output.status.success(), "{output:?}");
}

fn pristine_copy(repo: &Repo) -> Repo {
    let copy = Repo::empty("scope-pristine");
    copy_into(repo.root(), copy.root());
    copy
}

fn reset_to(repo: &Repo, pristine: &Repo) {
    for entry in fs::read_dir(repo.root()).unwrap_or_else(|e| unreachable!("{e}")) {
        let path = entry.unwrap_or_else(|e| unreachable!("{e}")).path();
        let is_dir = fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.is_dir());
        if is_dir {
            fs::remove_dir_all(&path).unwrap_or_else(|e| unreachable!("{e}"));
        } else {
            fs::remove_file(&path).unwrap_or_else(|e| unreachable!("{e}"));
        }
    }
    copy_into(pristine.root(), repo.root());
    assert_eq!(
        tree(repo.root()),
        tree(pristine.root()),
        "reset_to left the checkout off the baseline"
    );
}

fn tree(root: &Path) -> BTreeMap<String, String> {
    let mut entries = BTreeMap::new();
    walk(root, root, &mut entries);
    entries
}

fn walk(root: &Path, dir: &Path, entries: &mut BTreeMap<String, String>) {
    for entry in fs::read_dir(dir).unwrap_or_else(|e| unreachable!("{e}")) {
        let path = entry.unwrap_or_else(|e| unreachable!("{e}")).path();
        let relative = path
            .strip_prefix(root)
            .unwrap_or_else(|e| unreachable!("{e}"))
            .display()
            .to_string();
        let metadata = fs::symlink_metadata(&path).unwrap_or_else(|e| unreachable!("{e}"));
        let mode = metadata.permissions().mode() & 0o7777;
        let kind = if metadata.is_symlink() {
            let target = fs::read_link(&path).unwrap_or_else(|e| unreachable!("{e}"));
            format!("link {}", target.display())
        } else if metadata.is_dir() {
            walk(root, &path, entries);
            format!("dir {mode:o}")
        } else {
            let bytes = fs::read(&path).unwrap_or_else(|e| unreachable!("{e}"));
            format!("file {mode:o} {}", velnor_rust::sha256_hex(&bytes))
        };
        entries.insert(relative, kind);
    }
}

#[test]
fn changed_checkout_inputs_or_identity_refuse_scope_reuse() {
    let (repo, cache) = prepared();
    let pristine = pristine_copy(&repo);
    for change in [
        "manifest", "lock", "metadata", "added", "mode", "link", "type", "head", "job", "checkout",
        "root", "retarget", "removed",
    ] {
        reset_to(&repo, &pristine);
        match change {
            "manifest" => {
                repo.write_unstaged("Cargo.toml", "[package]\nname='changed'\nversion='0.1.0'\n");
            }
            "lock" => repo.write_unstaged(".velnor/rust-inputs.lock", "shared.txt\tapp\n"),
            "metadata" => repo.write_unstaged(".velnor/rust-metadata.json", "{}"),
            "added" => {
                repo.write_unstaged(".cargo/new-input", "new");
                repo.git(&["add", ".cargo/new-input"]);
            }
            "mode" => {
                fs::set_permissions(
                    repo.root().join("src/lib.rs"),
                    fs::Permissions::from_mode(0o755),
                )
                .unwrap_or_else(|e| unreachable!("{e}"));
            }
            "link" => {
                fs::remove_file(repo.root().join("src/lib.rs"))
                    .unwrap_or_else(|e| unreachable!("{e}"));
                std::os::unix::fs::symlink("../one/src/lib.rs", repo.root().join("src/lib.rs"))
                    .unwrap_or_else(|e| unreachable!("{e}"));
            }
            "type" => {
                fs::remove_file(repo.root().join("src/lib.rs"))
                    .unwrap_or_else(|e| unreachable!("{e}"));
                fs::create_dir(repo.root().join("src/lib.rs"))
                    .unwrap_or_else(|e| unreachable!("{e}"));
            }
            "retarget" => {
                fs::remove_file(repo.root().join("src/data-link"))
                    .unwrap_or_else(|e| unreachable!("{e}"));
                std::os::unix::fs::symlink("../other.bin", repo.root().join("src/data-link"))
                    .unwrap_or_else(|e| unreachable!("{e}"));
            }
            "removed" => {
                fs::remove_file(repo.root().join(".velnor/rust-metadata.json"))
                    .unwrap_or_else(|e| unreachable!("{e}"));
            }
            "head" => {
                repo.write_unstaged("notes.txt", "new commit");
                repo.git(&["add", "notes.txt"]);
                repo.commit();
            }
            "checkout" => {
                let other = fixture();
                other.write_unstaged("custom-target/.velnor/inputs.tsv", "stale");
                rejected(&other, &cache, "", "job");
                continue;
            }
            _ => {}
        }
        let output = rejected(
            &repo,
            &cache,
            if change == "root" { "one" } else { "" },
            if change == "job" { "other-job" } else { "job" },
        );
        if change == "root" {
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(
                stderr.contains("cache has no scope for this root"),
                "root refusal did not name the root identity: {stderr}"
            );
        }
    }
}

#[test]
fn a_valid_cached_scope_saves_current_modification_times() {
    let (repo, cache) = prepared();
    let current = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    repo.set_mtime("src/lib.rs", current);
    repo.write_unstaged("data.bin", "current bytes\n");
    repo.set_mtime("data.bin", current);
    let output = run(&repo, "", "job", "save", &cache);
    assert!(output.status.success(), "{output:?}");
    let snapshot = fs::read_to_string(repo.root().join("custom-target/.velnor/inputs.tsv"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let body = snapshot
        .split_once('\n')
        .map_or_else(|| unreachable!("{snapshot}"), |(_, body)| body);
    let saved = velnor_ci::inputs::parse_inputs(body).unwrap_or_else(|e| unreachable!("{e}"));
    let expected = Command::new("git")
        .arg("-C")
        .arg(repo.root())
        .args(["hash-object", "data.bin"])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(expected.status.success());
    assert_eq!(
        saved
            .iter()
            .find(|input| input.file.path == "data.bin")
            .map(|input| input.file.blob.as_str()),
        Some(String::from_utf8_lossy(&expected.stdout).trim())
    );
    repo.set_mtime("src/lib.rs", current + Duration::from_secs(5));
    repo.set_mtime("data.bin", current + Duration::from_secs(5));
    velnor_ci::io::restore_inputs(repo.root(), std::path::Path::new("custom-target"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(repo.mtime("src/lib.rs"), current);
    assert_eq!(repo.mtime("data.bin"), current);
    assert_eq!(
        fs::read_to_string(repo.root().join("data.bin")).unwrap_or_default(),
        "current bytes\n"
    );
}
