// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};
use std::path::{Path, PathBuf};

use velnor_ci::tools_local::{Inputs, Outcome, Verb, finish, message, node_label, run, valid_key};

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

fn sealed<T>(node: &Node, restore: impl FnOnce() -> T) -> T {
    let store = node.root.join("store");
    fs::set_permissions(&store, fs::Permissions::from_mode(0o555))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let outcome = restore();
    fs::set_permissions(&store, fs::Permissions::from_mode(0o755))
        .unwrap_or_else(|e| unreachable!("{e}"));
    outcome
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
        git_ref: if matches!(event, "push" | "schedule" | "workflow_dispatch") {
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
    let restored = sealed(&node, || {
        go(&node, Verb::Restore, KEY, "pull_request", "fresh")
    });
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
    let miss = || go(&node, Verb::Restore, KEY, "pull_request", "fresh");
    assert_eq!(sealed(&node, miss), Outcome::Miss);
    go(&node, Verb::Save, KEY, "push", "home");
    fs::remove_file(node.root.join("store/tools").join(KEY).join(".complete"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(sealed(&node, miss), Outcome::Miss);
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
    assert!(message(&Outcome::Miss, "node-a").contains("no entry"));
}

#[test]
fn an_untrusted_restore_refuses_a_store_the_job_can_write() {
    let node = node("tools-local-writable-seed");
    put(&node.root.join("home"));
    go(&node, Verb::Save, KEY, "push", "home");
    fs::create_dir_all(node.root.join("fresh")).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(
        go(&node, Verb::Restore, KEY, "pull_request", "fresh"),
        Outcome::Rejected("writable by this job")
    );
    assert!(!node.root.join("fresh/.rustup").exists());
    assert!(go(&node, Verb::Restore, KEY, "push", "fresh").hit());
}

#[test]
fn a_default_branch_dispatch_saves_like_a_push() {
    let node = node("tools-local-dispatch");
    put(&node.root.join("home"));
    assert!(matches!(
        go(&node, Verb::Save, KEY, "workflow_dispatch", "home"),
        Outcome::Saved { .. }
    ));
}

#[test]
fn concurrent_saves_of_one_key_publish_one_complete_entry() {
    let node = node("tools-local-concurrent");
    put(&node.root.join("home"));
    let outcomes: Vec<Outcome> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..4)
            .map(|_| scope.spawn(|| go(&node, Verb::Save, KEY, "push", "home")))
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().unwrap_or(Outcome::Failed("panicked")))
            .collect()
    });
    assert!(
        outcomes
            .iter()
            .all(|outcome| !matches!(outcome, Outcome::Failed(_) | Outcome::Rejected(_))),
        "{outcomes:?}"
    );
    assert!(outcomes.iter().any(|o| matches!(o, Outcome::Saved { .. })));
    let mut names: Vec<String> = fs::read_dir(node.root.join("store/tools"))
        .unwrap_or_else(|e| unreachable!("{e}"))
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(names, [KEY]);
    assert_eq!(
        go(&node, Verb::Save, KEY, "push", "home"),
        Outcome::AlreadySaved
    );
}

#[test]
fn a_key_another_job_is_saving_is_left_to_it() {
    let node = node("tools-local-busy");
    put(&node.root.join("home"));
    let lock = node.root.join("store/tools").join(format!(".lock-{KEY}"));
    fs::create_dir_all(&lock).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(go(&node, Verb::Save, KEY, "push", "home"), Outcome::Busy);
    assert!(!node.root.join("store/tools").join(KEY).exists());
    fs::remove_dir(&lock).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(matches!(
        go(&node, Verb::Save, KEY, "push", "home"),
        Outcome::Saved { .. }
    ));
    assert!(!lock.exists());
}

#[test]
fn abandoned_staging_trees_are_swept_and_active_ones_are_kept() {
    let node = node("tools-local-sweep");
    put(&node.root.join("home"));
    let tools = node.root.join("store/tools");
    let age = |path: &Path| {
        let old = std::time::SystemTime::now() - std::time::Duration::from_hours(2);
        fs::File::open(path)
            .and_then(|file| file.set_modified(old))
            .unwrap_or_else(|e| unreachable!("{e}"));
    };
    for name in [".staging-k0-dead", ".staging-k0-live", ".lock-k0"] {
        fs::create_dir_all(tools.join(name).join("payload"))
            .unwrap_or_else(|e| unreachable!("{e}"));
    }
    age(&tools.join(".staging-k0-dead"));
    age(&tools.join(".lock-k0"));
    assert!(matches!(
        go(&node, Verb::Save, KEY, "push", "home"),
        Outcome::Saved { .. }
    ));
    assert!(!tools.join(".staging-k0-dead").exists());
    assert!(!tools.join(".lock-k0").exists());
    assert!(tools.join(".staging-k0-live/payload").is_dir());
}

#[test]
fn a_failed_save_fails_the_step_but_nothing_else_does() {
    let failed = Outcome::Failed("the copy to the store failed");
    let error =
        finish(Verb::Save, &failed, "node-a").expect_err("a failed save must fail the step");
    assert!(error.to_string().contains("not saved"), "{error}");
    for outcome in [
        Outcome::NotOffered,
        Outcome::Miss,
        Outcome::AlreadySaved,
        Outcome::Busy,
        Outcome::Untrusted,
        Outcome::Rejected("world-writable"),
    ] {
        assert!(
            finish(Verb::Save, &outcome, "node-a").is_ok(),
            "{outcome:?}"
        );
    }
    assert!(finish(Verb::Restore, &failed, "node-a").is_ok());
}

#[test]
fn a_save_that_cannot_copy_is_an_error_and_leaves_nothing_behind() {
    let node = node("tools-local-failed");
    fs::create_dir_all(node.root.join("bare")).unwrap_or_else(|e| unreachable!("{e}"));
    let outcome = go(&node, Verb::Save, KEY, "push", "bare");
    assert_eq!(outcome, Outcome::Failed("the copy to the store failed"));
    assert!(message(&outcome, "node-a").starts_with("::error::"));
    let left: Vec<_> = fs::read_dir(node.root.join("store/tools"))
        .unwrap_or_else(|e| unreachable!("{e}"))
        .flatten()
        .collect();
    assert!(left.is_empty(), "{left:?}");
}

#[test]
fn every_tools_line_names_the_node_and_the_name_cannot_break_a_workflow_command() {
    assert_eq!(
        node_label(Some("velnor-240"), Some("runner-pod-7")),
        "velnor-240"
    );
    assert_eq!(node_label(None, Some("runner-pod-7")), "runner-pod-7");
    assert_eq!(node_label(Some("  "), Some("runner-pod-7")), "runner-pod-7");
    assert_eq!(node_label(Some("a::b c"), None), "a__b_c");
    assert_eq!(node_label(None, None), "unknown");
    let long = node_label(Some(&"n".repeat(500)), None);
    assert!(long.len() <= 253, "{}", long.len());
    for outcome in [
        Outcome::NotOffered,
        Outcome::BadKey,
        Outcome::Miss,
        Outcome::Restored {
            from: PathBuf::from("/store/tools/k"),
        },
        Outcome::Rejected("world-writable"),
        Outcome::Saved {
            to: PathBuf::from("/store/tools/k"),
        },
        Outcome::AlreadySaved,
        Outcome::Busy,
        Outcome::Failed("the copy to the store failed"),
        Outcome::Untrusted,
    ] {
        let line = message(&outcome, "velnor-240");
        assert!(
            line.ends_with(" node=velnor-240\n"),
            "{outcome:?}: {line:?}"
        );
        assert_eq!(line.matches('\n').count(), 1, "{outcome:?}: {line:?}");
    }
    let error = finish(Verb::Save, &Outcome::Failed("x"), "velnor-240").expect_err("failed save");
    assert!(error.to_string().contains("node=velnor-240"), "{error}");
}
