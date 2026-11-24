// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs::{self, File, FileTimes};
use std::path::Path;
use std::process::Command;
use std::time::{Duration, SystemTime};
use velnor_test_support::real_cargo_str;

use crate::io::Repo;

fn run(root: &Path, program: &str, args: &[&str]) -> String {
    let output = Command::new(program)
        .args(args)
        .current_dir(root)
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTC_WRAPPER")
        .env_remove("RUSTC_WORKSPACE_WRAPPER")
        .env("CARGO_TERM_COLOR", "never")
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.status.success(), "{program} {args:?}: {text}");
    text
}

fn cargo(root: &Path) -> String {
    run(root, &real_cargo_str(), &["build", "--offline", "-v"])
}

fn set_time(path: &Path, time: SystemTime) {
    File::open(path)
        .and_then(|file| file.set_times(FileTimes::new().set_modified(time)))
        .unwrap_or_else(|e| unreachable!("{}: {e}", path.display()));
}

fn check_out(dir: &Path, time: SystemTime) {
    for entry in fs::read_dir(dir).unwrap_or_else(|e| unreachable!("{e}")) {
        let path = entry.unwrap_or_else(|e| unreachable!("{e}")).path();
        if path.ends_with(".git") || path.ends_with("target") {
            continue;
        }
        if path.is_dir() {
            check_out(&path, time);
        }
        set_time(&path, time);
    }
}

fn restored_build(name: &str, edit: impl FnOnce(&Repo, SystemTime)) -> (Repo, SystemTime, String) {
    let repo = Repo::new(name);
    repo.write_all(&[
        ("Cargo.toml", "[package]\nname='app'\nversion='0.1.0'\nedition='2024'\n[dependencies]\nwatcher={path='watcher'}\n"),
        ("src/main.rs", "fn main() { watcher::value(); }\n"),
        ("watcher/Cargo.toml", "[package]\nname='watcher'\nversion='0.1.0'\nedition='2024'\n"),
        ("watcher/build.rs", "fn main() { println!(\"cargo:rerun-if-changed=assets\"); }\n"),
        ("watcher/src/lib.rs", "pub fn value() {}\n"),
        ("watcher/assets/one.txt", "one\n"),
        ("watcher/assets/nested/two.txt", "two\n"),
    ]);
    cargo(repo.root());
    repo.git(&["add", "--all"]);
    repo.commit();
    run(
        repo.root(),
        env!("CARGO_BIN_EXE_velnor-ci"),
        &["inputs", "save"],
    );
    let checkout = SystemTime::now() + Duration::from_secs(10);
    check_out(repo.root(), checkout);
    edit(&repo, checkout);
    run(
        repo.root(),
        env!("CARGO_BIN_EXE_velnor-ci"),
        &["inputs", "restore"],
    );
    let build = cargo(repo.root());
    (repo, checkout, build)
}

#[test]
fn a_build_script_watching_an_unchanged_tracked_folder_stays_fresh_after_restore() {
    let (_repo, _, build) = restored_build("restore-dirs-fresh", |_, _| {});
    assert!(build.contains("Fresh watcher "), "{build}");
    assert!(!build.contains("Compiling "), "{build}");
}

#[test]
fn a_build_script_watching_a_folder_with_a_deleted_or_added_tracked_file_reruns() {
    for (name, added) in [
        ("restore-dirs-deleted", false),
        ("restore-dirs-added", true),
    ] {
        let (_repo, _, build) = restored_build(name, |repo, _| {
            if added {
                repo.write("watcher/assets/nested/three.txt", "three\n");
                repo.set_mtime("watcher/assets/nested/three.txt", SystemTime::UNIX_EPOCH);
            } else {
                repo.git(&["rm", "--quiet", "watcher/assets/nested/two.txt"]);
            }
        });
        assert!(build.contains("Compiling watcher "), "{name}: {build}");
    }
}

#[test]
fn a_build_script_watching_a_folder_with_a_changed_file_reruns_and_keeps_the_checkout_time() {
    let (repo, checkout, build) = restored_build("restore-dirs-changed", |repo, checkout| {
        repo.write("watcher/assets/one.txt", "changed\n");
        repo.set_mtime("watcher/assets/one.txt", checkout);
    });
    assert!(build.contains("Compiling watcher "), "{build}");
    assert_eq!(repo.mtime("watcher/assets"), checkout);
}
