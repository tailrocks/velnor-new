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

fn inputs(root: &Path, operation: &str) {
    run(
        root,
        env!("CARGO_BIN_EXE_velnor-ci"),
        &["inputs", operation],
    );
}

fn restored_build(
    name: &str,
    generated: bool,
    edit: impl FnOnce(&Repo, SystemTime),
) -> (Repo, SystemTime, String) {
    let repo = Repo::new(name);
    repo.write_unstaged(".git/info/exclude", "generated.txt\n");
    repo.write_all(&[
        ("Cargo.toml", "[package]\nname='app'\nversion='0.1.0'\nedition='2024'\n[dependencies]\nwatcher={path='watcher'}\n"),
        ("src/main.rs", "fn main() { watcher::value(); }\n"),
        ("watcher/Cargo.toml", "[package]\nname='watcher'\nversion='0.1.0'\nedition='2024'\n"),
        ("watcher/build.rs", "fn main() { println!(\"cargo:rerun-if-changed=assets\"); }\n"),
        ("watcher/src/lib.rs", "pub fn value() {}\n"),
        ("watcher/assets/one.txt", "one\n"),
        ("watcher/assets/nested/two.txt", "two\n"),
    ]);
    if generated {
        repo.write_unstaged("watcher/assets/nested/generated.txt", "generated\n");
    }
    cargo(repo.root());
    repo.git(&["add", "--all"]);
    repo.commit();
    inputs(repo.root(), "save");
    let checkout = SystemTime::now() + Duration::from_secs(10);
    check_out(repo.root(), checkout);
    edit(&repo, checkout);
    inputs(repo.root(), "restore");
    let build = cargo(repo.root());
    (repo, checkout, build)
}

fn reruns(name: &str, generated: bool, edit: impl FnOnce(&Repo)) {
    let (_repo, _, build) = restored_build(name, generated, |repo, _| edit(repo));
    assert!(build.contains("Compiling watcher "), "{name}: {build}");
}

fn add_old(repo: &Repo, path: &str) {
    repo.write_unstaged(path, "new\n");
    set_time(&repo.root().join(path), SystemTime::UNIX_EPOCH);
}

#[test]
fn a_build_script_watching_an_unchanged_tracked_folder_stays_fresh_after_restore() {
    let (_repo, _, build) = restored_build("restore-dirs-fresh", false, |_, _| {});
    assert!(build.contains("Fresh watcher "), "{build}");
    assert!(!build.contains("Compiling "), "{build}");
}

#[test]
fn a_build_script_watching_a_folder_with_a_deleted_tracked_file_reruns() {
    reruns("restore-dirs-deleted", false, |repo| {
        repo.git(&["rm", "--quiet", "watcher/assets/nested/two.txt"]);
    });
}

#[test]
fn a_build_script_watching_a_folder_with_an_added_tracked_file_reruns() {
    reruns("restore-dirs-added", false, |repo| {
        add_old(repo, "watcher/assets/nested/three.txt");
        repo.git(&["add", "--all"]);
    });
}

#[test]
fn a_build_script_watching_a_folder_with_an_added_untracked_file_reruns() {
    reruns("restore-dirs-untracked", false, |repo| {
        add_old(repo, "watcher/assets/nested/three.txt");
    });
}

#[test]
fn a_build_script_watching_a_folder_with_an_added_ignored_file_reruns() {
    reruns("restore-dirs-ignored", false, |repo| {
        add_old(repo, "watcher/assets/nested/generated.txt");
    });
}

#[test]
fn a_build_script_watching_a_folder_with_an_added_empty_folder_reruns() {
    reruns("restore-dirs-empty", false, |repo| {
        let empty = repo.root().join("watcher/assets/nested/empty");
        fs::create_dir(&empty).unwrap_or_else(|e| unreachable!("{e}"));
        set_time(&empty, SystemTime::UNIX_EPOCH);
    });
}

#[test]
fn a_build_script_watching_a_folder_whose_ignored_file_is_gone_at_restore_reruns() {
    reruns("restore-dirs-gone", true, |repo| {
        fs::remove_file(repo.root().join("watcher/assets/nested/generated.txt"))
            .unwrap_or_else(|e| unreachable!("{e}"));
    });
}

#[test]
fn a_build_script_watching_a_folder_whose_ignored_file_was_replaced_reruns() {
    reruns("restore-dirs-replaced", true, |repo| {
        add_old(repo, "watcher/assets/nested/generated.txt");
    });
}

#[test]
fn a_build_script_watching_a_folder_with_a_changed_file_reruns_and_keeps_the_checkout_time() {
    let (repo, checkout, build) =
        restored_build("restore-dirs-changed", false, |repo, checkout| {
            repo.write("watcher/assets/one.txt", "changed\n");
            repo.set_mtime("watcher/assets/one.txt", checkout);
        });
    assert!(build.contains("Compiling watcher "), "{build}");
    assert_eq!(repo.mtime("watcher/assets"), checkout);
}

#[test]
fn a_scoped_restore_keeps_the_checkout_time_of_a_folder_partly_out_of_scope() {
    let repo = Repo::new("restore-dirs-scope");
    repo.write_all(&[
        ("mise.toml", "[tools]\nrust='1.99.1'\n"),
        (".gitignore", "**/target/\n"),
        (".velnor/rust-ci.toml", "exclude = [\"fixtures/**\"]\n"),
        ("app/Cargo.toml", "[package]\nname='app'\nversion='0.1.0'\nedition='2024'\n[dependencies]\nsibling={path='../fixtures/sibling'}\n"),
        ("app/src/main.rs", "fn main() { sibling::value(); }\n"),
        ("fixtures/sibling/Cargo.toml", "[package]\nname='sibling'\nversion='0.1.0'\nedition='2024'\n"),
        ("fixtures/sibling/src/lib.rs", "pub fn value() {}\n"),
        ("fixtures/notes.txt", "out of scope\n"),
    ]);
    let app = repo.root().join("app");
    cargo(&app);
    velnor_gen::io::write_metadata(repo.root()).unwrap_or_else(|e| unreachable!("{e}"));
    repo.git(&["add", "--all"]);
    inputs(&app, "save");
    let checkout = SystemTime::now() + Duration::from_secs(10);
    check_out(repo.root(), checkout);
    inputs(&app, "restore");
    for restored in ["fixtures/sibling", "fixtures/sibling/src", "app/src"] {
        assert!(repo.mtime(restored) < checkout, "{restored}");
    }
    assert_eq!(repo.mtime("fixtures"), checkout);
}
