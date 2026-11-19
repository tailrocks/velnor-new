// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};
use std::path::{Path, PathBuf};

use velnor_ci::tools_local::{Inputs, Outcome, Verb, message, run, valid_key};

use crate::io::Repo;

const KEY: &str = "velnor-tools-Linux-X64-mise2026.11.10-abc123";

struct Node {
    _repo: Repo,
    root: PathBuf,
}

fn node(name: &str) -> Node {
    let repo = Repo::empty(name);
    let root = fs::canonicalize(repo.root()).unwrap_or_else(|e| unreachable!("{e}"));
    for dir in ["store", "home"] {
        fs::create_dir_all(root.join(dir)).unwrap_or_else(|e| unreachable!("{e}"));
    }
    Node { _repo: repo, root }
}

fn put(home: &Path) {
    let tool = home.join(".local/share/mise/installs/cosign/3.0.0/bin");
    fs::create_dir_all(&tool).unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(tool.join("cosign"), "binary").unwrap_or_else(|e| unreachable!("{e}"));
    fs::set_permissions(tool.join("cosign"), fs::Permissions::from_mode(0o755))
        .unwrap_or_else(|e| unreachable!("{e}"));
    symlink(
        "3.0.0",
        home.join(".local/share/mise/installs/cosign/latest"),
    )
    .unwrap_or_else(|e| unreachable!("{e}"));
    let rustup = home.join(".rustup/toolchains/stable");
    fs::create_dir_all(&rustup).unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(rustup.join("rustc"), "rustc").unwrap_or_else(|e| unreachable!("{e}"));
}

fn go(node: &Node, verb: Verb, key: &str, event: &str, home: &str) -> Outcome {
    let store = node.root.join("store").display().to_string();
    let home = node.root.join(home);
    let owner = fs::metadata(&home).map_or(0, |metadata| metadata.uid());
    run(&Inputs {
        verb,
        key,
        event_name: event,
        git_ref: if event == "push" {
            "refs/heads/main"
        } else {
            "refs/pull/1/merge"
        },
        default_branch: Some("main"),
        shared_dir: Some(&store),
        seed_dir: Some(&store),
        home: &home,
        owner,
    })
}

#[test]
fn a_trusted_save_is_restored_whole_on_a_pull_request() {
    let node = node("tools-local-roundtrip");
    put(&node.root.join("home"));
    let saved = go(&node, Verb::Save, KEY, "push", "home");
    assert!(matches!(saved, Outcome::Saved { .. }), "{saved:?}");
    fs::create_dir_all(node.root.join("fresh")).unwrap_or_else(|e| unreachable!("{e}"));
    let restored = go(&node, Verb::Restore, KEY, "pull_request", "fresh");
    assert!(restored.hit(), "{restored:?}");
    let cosign = node
        .root
        .join("fresh/.local/share/mise/installs/cosign/3.0.0/bin/cosign");
    assert_eq!(fs::read_to_string(&cosign).unwrap_or_default(), "binary");
    assert_eq!(fs::metadata(&cosign).map_or(0, |m| m.mode() & 0o777), 0o755);
    assert_eq!(
        fs::read_link(
            node.root
                .join("fresh/.local/share/mise/installs/cosign/latest")
        )
        .ok(),
        Some(PathBuf::from("3.0.0"))
    );
    assert!(
        node.root
            .join("fresh/.rustup/toolchains/stable/rustc")
            .is_file()
    );
}

#[test]
fn an_untrusted_event_never_saves_and_a_second_trusted_save_changes_nothing() {
    let node = node("tools-local-untrusted");
    put(&node.root.join("home"));
    assert_eq!(
        go(&node, Verb::Save, KEY, "pull_request", "home"),
        Outcome::Untrusted
    );
    assert!(!node.root.join("store/tools").exists());
    assert!(matches!(
        go(&node, Verb::Save, KEY, "push", "home"),
        Outcome::Saved { .. }
    ));
    assert_eq!(
        go(&node, Verb::Save, KEY, "push", "home"),
        Outcome::AlreadySaved
    );
}

#[test]
fn an_entry_without_its_marker_or_for_another_key_is_a_miss_and_copies_nothing() {
    let node = node("tools-local-miss");
    put(&node.root.join("home"));
    fs::create_dir_all(node.root.join("fresh")).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(
        go(&node, Verb::Restore, KEY, "pull_request", "fresh"),
        Outcome::Miss
    );
    go(&node, Verb::Save, KEY, "push", "home");
    fs::remove_file(node.root.join("store/tools").join(KEY).join(".complete"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(
        go(&node, Verb::Restore, KEY, "pull_request", "fresh"),
        Outcome::Miss
    );
    assert_eq!(
        go(&node, Verb::Restore, "other-key", "push", "fresh"),
        Outcome::Miss
    );
    assert!(!node.root.join("fresh/.rustup").exists());
}

#[test]
fn a_key_that_is_not_a_plain_name_is_refused() {
    let node = node("tools-local-badkey");
    for key in ["", ".hidden", "a/b", "../x", "a b", "a;b", &"k".repeat(201)] {
        assert!(!valid_key(key), "{key}");
        assert_eq!(
            go(&node, Verb::Restore, key, "push", "home"),
            Outcome::BadKey
        );
    }
    assert!(valid_key(KEY));
}

#[test]
fn the_store_keeps_the_two_newest_keys() {
    let node = node("tools-local-prune");
    put(&node.root.join("home"));
    for key in ["k1", "k2", "k3"] {
        assert!(matches!(
            go(&node, Verb::Save, key, "push", "home"),
            Outcome::Saved { .. }
        ));
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let mut left: Vec<String> = fs::read_dir(node.root.join("store/tools"))
        .unwrap_or_else(|e| unreachable!("{e}"))
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    left.sort();
    assert_eq!(left, ["k2", "k3"]);
}

#[test]
fn a_world_writable_or_missing_store_is_not_used() {
    let node = node("tools-local-unsafe");
    put(&node.root.join("home"));
    go(&node, Verb::Save, KEY, "push", "home");
    fs::set_permissions(node.root.join("store"), fs::Permissions::from_mode(0o777))
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(matches!(
        go(&node, Verb::Restore, KEY, "pull_request", "home"),
        Outcome::Rejected(_)
    ));
    assert!(message(&Outcome::Miss).contains("no entry"));
}
