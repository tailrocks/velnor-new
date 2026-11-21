// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::os::unix::fs::symlink;
use std::time::{Duration, UNIX_EPOCH};

use super::{audit, inventory::build, own_units, unit, workspace_below_the_root};
use crate::io::Repo;

const OUTPUT: &str = "rust/target/debug/build/lib-f000000000000008/output";

fn declared_link(target: &str) -> Repo {
    let repo = workspace_below_the_root();
    own_units(&repo, "rust/target/debug", &[]);
    unit(&repo, "elsewhere/inner/value.txt", "inner");
    unit(&repo, "elsewhere/other/value.txt", "other");
    unit(&repo, "elsewhere/file", "file");
    let links = repo.root().join("shared/dir");
    fs::create_dir_all(&links).unwrap_or_else(|e| unreachable!("{e}"));
    if target == "base" {
        symlink("../../elsewhere/inner", links.join("base"))
            .unwrap_or_else(|e| unreachable!("{e}"));
    }
    let root = repo.root().display().to_string();
    symlink(target.replace("{root}", &root), links.join("link"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    unit(&repo, OUTPUT, "cargo:rerun-if-changed=../../shared\n");
    repo
}

fn add_entry(repo: &Repo, path: &str) {
    unit(repo, path, "new");
    repo.set_mtime(path, UNIX_EPOCH + Duration::from_secs(1_000_000_000));
}

fn is_stale(repo: &Repo) -> bool {
    let written = audit(repo, &["--write"]);
    let lock = repo.root().join(".velnor/rust-inputs.lock");
    match written.status.code() {
        Some(0) => {
            fs::remove_file(lock).unwrap_or_else(|e| unreachable!("{e}"));
            false
        }
        Some(1) => {
            eprintln!("{}", String::from_utf8_lossy(&written.stderr));
            assert!(!lock.exists());
            true
        }
        _ => unreachable!("{written:?}"),
    }
}

#[test]
fn a_link_naming_a_directory_in_any_spelling_is_walked_as_that_directory() {
    for (name, target) in [
        ("plain", "../../elsewhere/inner"),
        ("up", "../../elsewhere/other/../inner"),
        ("dots", "./../../elsewhere/./inner/../inner/."),
        ("absolute", "{root}/elsewhere/inner"),
        ("slashed", "../../elsewhere/inner/"),
        ("hop", "base"),
    ] {
        let repo = declared_link(target);
        build(&repo, OUTPUT, None);
        add_entry(&repo, "elsewhere/inner/new.txt");
        assert!(is_stale(&repo), "{name}");
    }
}

#[test]
fn a_link_naming_no_directory_is_neither_an_error_nor_walked() {
    for (name, target) in [
        ("file", "../../elsewhere/file"),
        ("dangling", "nowhere"),
        ("through-file", "../../elsewhere/file/more"),
        ("loop", "link"),
    ] {
        let repo = declared_link(target);
        build(&repo, OUTPUT, None);
        add_entry(&repo, "elsewhere/inner/new.txt");
        assert!(!is_stale(&repo), "{name}");
    }
}

#[test]
fn a_link_that_named_no_directory_is_stale_once_a_directory_appears_there() {
    for (name, target, replaced) in [
        ("file", "../../elsewhere/file", "elsewhere/file"),
        (
            "through-file",
            "../../elsewhere/file/more",
            "elsewhere/file",
        ),
        ("dangling", "nowhere", "shared/dir/nowhere"),
    ] {
        let repo = declared_link(target);
        build(&repo, OUTPUT, None);
        let path = repo.root().join(replaced);
        if path.is_file() {
            fs::remove_file(&path).unwrap_or_else(|e| unreachable!("{e}"));
        }
        fs::create_dir_all(path.join("more")).unwrap_or_else(|e| unreachable!("{e}"));
        add_entry(&repo, &format!("{replaced}/more/value.txt"));
        assert!(is_stale(&repo), "{name}");
    }
}
