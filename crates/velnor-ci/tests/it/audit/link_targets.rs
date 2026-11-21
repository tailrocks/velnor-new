// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::os::unix::fs::symlink;
use std::time::{Duration, UNIX_EPOCH};

use super::{audit, inventory::build, own_units, unit, workspace_below_the_root};
use crate::io::Repo;

const OUTPUT: &str = "rust/target/debug/build/lib-f000000000000008/output";
const STALE_INPUT: &str = "output stale: input ../../shared changed after it ran";

fn declared_link(target: &str) -> Repo {
    let repo = workspace_below_the_root();
    own_units(&repo, "rust/target/debug", &[]);
    unit(&repo, "elsewhere/inner/value.txt", "inner");
    unit(&repo, "elsewhere/other/value.txt", "other");
    unit(&repo, "elsewhere/file", "file");
    let links = repo.root().join("shared/dir");
    fs::create_dir_all(&links).unwrap_or_else(|e| unreachable!("{e}"));
    if target == "../../elsewhere/hop" {
        symlink("inner", repo.root().join("elsewhere/hop")).unwrap_or_else(|e| unreachable!("{e}"));
    }
    let root = repo.root().display().to_string();
    symlink(target.replace("{root}", &root), links.join("link"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    unit(&repo, OUTPUT, "cargo:rerun-if-changed=../../shared\n");
    unit(
        &repo,
        "rust/target/debug/.fingerprint/lib-f000000000000008/run-build-script-build-script-build",
        "",
    );
    repo
}

fn add_entry(repo: &Repo, path: &str) {
    unit(repo, path, "new");
    repo.set_mtime(path, UNIX_EPOCH + Duration::from_secs(1_000_000_000));
}

fn assert_fresh(repo: &Repo, name: &str) {
    let written = audit(repo, &["--write"]);
    assert!(
        written.status.success(),
        "{name}: {}",
        String::from_utf8_lossy(&written.stderr)
    );
    let lock = repo.root().join(".velnor/rust-inputs.lock");
    assert_eq!(
        fs::read_to_string(&lock).unwrap_or_else(|e| unreachable!("{e}")),
        "shared\tlib\n",
        "{name}"
    );
    fs::remove_file(lock).unwrap_or_else(|e| unreachable!("{e}"));
}

fn assert_declared_input_stale(repo: &Repo, name: &str) {
    let refused = audit(repo, &["--write"]);
    let stderr = String::from_utf8_lossy(&refused.stderr);
    assert_eq!(refused.status.code(), Some(1), "{name}: {stderr}");
    assert!(
        stderr.contains(STALE_INPUT),
        "{name}: missing {STALE_INPUT:?} in {stderr}"
    );
    assert!(!repo.root().join(".velnor/rust-inputs.lock").exists());
}

#[test]
fn a_new_entry_under_a_link_to_a_directory_makes_the_declared_input_stale_in_any_spelling() {
    for (name, target) in [
        ("plain", "../../elsewhere/inner"),
        ("up", "../../elsewhere/other/../inner"),
        ("dots", "./../../elsewhere/./inner/../inner/."),
        ("absolute", "{root}/elsewhere/inner"),
        ("slashed", "../../elsewhere/inner/"),
        ("hop", "../../elsewhere/hop"),
    ] {
        let repo = declared_link(target);
        build(&repo, OUTPUT, None);
        assert_fresh(&repo, name);
        add_entry(&repo, "elsewhere/inner/new.txt");
        assert_declared_input_stale(&repo, name);
    }
}

#[test]
fn a_new_entry_beside_a_link_naming_no_directory_leaves_the_declared_input_fresh() {
    for (name, target) in [
        ("file", "../../elsewhere/file"),
        ("dangling", "nowhere"),
        ("through-file", "../../elsewhere/file/more"),
        ("loop", "link"),
    ] {
        let repo = declared_link(target);
        build(&repo, OUTPUT, None);
        assert_fresh(&repo, name);
        add_entry(&repo, "elsewhere/inner/new.txt");
        assert_fresh(&repo, name);
    }
}

#[test]
fn a_directory_appearing_where_a_link_named_none_makes_the_declared_input_stale() {
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
        assert_fresh(&repo, name);
        let path = repo.root().join(replaced);
        if path.is_file() {
            fs::remove_file(&path).unwrap_or_else(|e| unreachable!("{e}"));
        }
        fs::create_dir_all(path.join("more")).unwrap_or_else(|e| unreachable!("{e}"));
        add_entry(&repo, &format!("{replaced}/more/value.txt"));
        assert_declared_input_stale(&repo, name);
    }
}
