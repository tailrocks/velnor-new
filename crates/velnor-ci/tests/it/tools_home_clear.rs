// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::Command;

use velnor_ci::tools_local::{Inputs, Outcome, Verb, run};
use velnor_ci::tools_verify::{Anchor, Recording, Verdict, check, digest, record};

use crate::io::Repo;

const KEY: &str = "velnor-tools-Linux-X64-mise2026.11.10-abc123";

struct Node {
    _repo: Repo,
    root: PathBuf,
}

fn node(name: &str) -> Node {
    let repo = Repo::empty(name);
    let root = fs::canonicalize(repo.root()).unwrap_or_else(|e| unreachable!("{e}"));
    for dir in ["store", "stored", "baked"] {
        fs::create_dir_all(root.join(dir)).unwrap_or_else(|e| unreachable!("{e}"));
    }
    Node { _repo: repo, root }
}

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap_or(path)).unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(path, text).unwrap_or_else(|e| unreachable!("{e}"));
}

fn stored_tools(home: &Path) {
    write(
        &home.join(".local/share/mise/installs/cosign/3.0.0/bin/cosign"),
        "binary",
    );
    write(&home.join(".rustup/toolchains/stable/bin/rustc"), "rustc");
}

fn baked_extras(home: &Path) {
    stored_tools(home);
    write(
        &home.join(".local/share/mise/installs/cosign/2.9.0/bin/cosign"),
        "old",
    );
    write(
        &home.join(".local/share/mise/downloads/cosign/x.tar"),
        "tar",
    );
    write(&home.join(".local/share/mise/shims/cosign"), "shim");
    write(
        &home.join(".rustup/toolchains/nightly/bin/rustc"),
        "nightly",
    );
}

fn go(node: &Node, verb: Verb, home: &Path) -> Outcome {
    let store = node.root.join("store").display().to_string();
    run(&Inputs {
        verb,
        key: KEY,
        event_name: "push",
        git_ref: "refs/heads/main",
        default_branch: Some("main"),
        shared_dir: Some(&store),
        seed_dir: Some(&store),
        home,
        replace: false,
    })
}

fn saved(node: &Node) {
    stored_tools(&node.root.join("stored"));
    let outcome = go(node, Verb::Save, &node.root.join("stored"));
    assert!(matches!(outcome, Outcome::Saved { .. }), "{outcome:?}");
}

#[test]
fn a_restore_over_a_baked_home_leaves_exactly_the_stored_tree() {
    let node = node("tools-home-clear-baked");
    saved(&node);
    let baked = node.root.join("baked");
    baked_extras(&baked);
    assert_ne!(digest(&baked), digest(&node.root.join("stored")));
    let file = node.root.join("anchor/digest");
    let recording = Recording {
        anchor: Anchor(""),
        file: &file,
        trusted: true,
        verified: false,
    };
    let Ok(Verdict::Recorded(anchor)) = record(&node.root.join("stored"), &recording) else {
        unreachable!("the stored tree was not recorded")
    };
    let restored = go(&node, Verb::Restore, &baked);
    assert!(restored.hit(), "{restored:?}");
    let anchor = Anchor(&anchor);
    let file = baked.join("velnor-tools-anchor/digest");
    assert_eq!(check(&baked, &anchor, &file).ok(), Some(Verdict::Verified));
    assert_eq!(digest(&baked), digest(&node.root.join("stored")));
    assert!(
        !baked
            .join(".local/share/mise/installs/cosign/2.9.0")
            .exists()
    );
    assert!(!baked.join(".rustup/toolchains/nightly").exists());
}

#[test]
fn a_restore_leaves_what_the_helper_does_not_own() {
    let node = node("tools-home-clear-neighbours");
    saved(&node);
    let baked = node.root.join("baked");
    baked_extras(&baked);
    write(&baked.join(".cargo/bin/cargo"), "cargo");
    write(&baked.join(".cache/mise/keep"), "cache");
    write(&baked.join(".local/share/other/keep"), "other");
    assert!(go(&node, Verb::Restore, &baked).hit());
    for kept in [
        ".cargo/bin/cargo",
        ".cache/mise/keep",
        ".local/share/other/keep",
    ] {
        assert!(baked.join(kept).is_file(), "{kept}");
    }
}

#[test]
fn a_symlinked_tool_directory_is_refused_and_never_followed() {
    let node = node("tools-home-clear-symlink");
    saved(&node);
    let outside = node.root.join("outside");
    write(&outside.join("precious"), "keep");
    let baked = node.root.join("baked");
    stored_tools(&baked);
    fs::remove_dir_all(baked.join(".rustup")).unwrap_or_else(|e| unreachable!("{e}"));
    symlink(&outside, baked.join(".rustup")).unwrap_or_else(|e| unreachable!("{e}"));
    let outcome = go(&node, Verb::Restore, &baked);
    assert!(matches!(outcome, Outcome::Rejected(_)), "{outcome:?}");
    assert_eq!(
        fs::read_to_string(outside.join("precious")).unwrap_or_default(),
        "keep"
    );
    assert!(!outside.join("toolchains").exists());
    assert!(
        fs::symlink_metadata(baked.join(".rustup"))
            .is_ok_and(|metadata| metadata.file_type().is_symlink())
    );
}

#[test]
fn a_symlinked_parent_of_a_tool_directory_is_refused_and_never_followed() {
    let node = node("tools-home-clear-parent");
    saved(&node);
    let outside = node.root.join("outside");
    write(&outside.join("share/mise/precious"), "keep");
    let baked = node.root.join("baked");
    fs::create_dir_all(&baked).unwrap_or_else(|e| unreachable!("{e}"));
    symlink(&outside, baked.join(".local")).unwrap_or_else(|e| unreachable!("{e}"));
    let outcome = go(&node, Verb::Restore, &baked);
    assert!(matches!(outcome, Outcome::Rejected(_)), "{outcome:?}");
    assert_eq!(
        fs::read_to_string(outside.join("share/mise/precious")).unwrap_or_default(),
        "keep"
    );
    assert!(!outside.join("share/mise/installs").exists());
}

#[test]
fn a_home_that_is_the_filesystem_root_or_empty_is_refused() {
    let node = node("tools-home-clear-root");
    saved(&node);
    for home in ["/", ""] {
        let outcome = go(&node, Verb::Restore, Path::new(home));
        assert!(
            matches!(outcome, Outcome::Rejected(_)),
            "{home:?}: {outcome:?}"
        );
    }
}

#[test]
fn clear_empties_exactly_the_restored_directories_without_a_key() {
    let node = node("tools-home-clear-verb");
    let baked = node.root.join("baked");
    baked_extras(&baked);
    write(&baked.join(".cargo/bin/cargo"), "cargo");
    write(&baked.join(".local/share/other/keep"), "other");
    let outcome = go(&node, Verb::Clear, &baked);
    assert_eq!(outcome, Outcome::Cleared);
    assert!(!baked.join(".local/share/mise").exists());
    assert!(!baked.join(".rustup").exists());
    assert!(baked.join(".cargo/bin/cargo").is_file());
    assert!(baked.join(".local/share/other/keep").is_file());
    assert_eq!(go(&node, Verb::Clear, &baked), Outcome::Cleared);
}

#[test]
fn clear_refuses_a_symlink_and_a_root_home_and_deletes_nothing() {
    let node = node("tools-home-clear-verb-refused");
    let outside = node.root.join("outside");
    write(&outside.join("precious"), "keep");
    let baked = node.root.join("baked");
    write(&baked.join(".local/share/mise/installs/x"), "x");
    symlink(&outside, baked.join(".rustup")).unwrap_or_else(|e| unreachable!("{e}"));
    let outcome = go(&node, Verb::Clear, &baked);
    assert!(matches!(outcome, Outcome::Rejected(_)), "{outcome:?}");
    assert!(outside.join("precious").is_file());
    assert!(baked.join(".local/share/mise/installs/x").is_file());
    for home in ["/", ""] {
        let outcome = go(&node, Verb::Clear, Path::new(home));
        assert!(
            matches!(outcome, Outcome::Rejected(_)),
            "{home:?}: {outcome:?}"
        );
    }
}

#[test]
fn a_symlinked_home_is_refused_and_its_target_is_never_cleared() {
    let node = node("tools-home-clear-linked-home");
    saved(&node);
    let real = node.root.join("real-home");
    baked_extras(&real);
    let linked = node.root.join("linked-home");
    symlink(&real, &linked).unwrap_or_else(|e| unreachable!("{e}"));
    let clear = go(&node, Verb::Clear, &linked);
    assert!(matches!(clear, Outcome::Rejected(_)), "{clear:?}");
    let restore = go(&node, Verb::Restore, &linked);
    assert!(matches!(restore, Outcome::Rejected(_)), "{restore:?}");
    assert!(
        real.join(".local/share/mise/installs/cosign/2.9.0/bin/cosign")
            .is_file()
    );
    assert!(real.join(".rustup/toolchains/nightly/bin/rustc").is_file());
}

#[test]
fn a_symlinked_home_with_a_trailing_separator_is_refused_too() {
    let node = node("tools-home-clear-linked-home-slash");
    saved(&node);
    let real = node.root.join("real-home");
    baked_extras(&real);
    let linked = node.root.join("linked-home");
    symlink(&real, &linked).unwrap_or_else(|e| unreachable!("{e}"));
    let slashed = PathBuf::from(format!("{}/", linked.display()));
    let clear = go(&node, Verb::Clear, &slashed);
    assert!(matches!(clear, Outcome::Rejected(_)), "{clear:?}");
    let restore = go(&node, Verb::Restore, &slashed);
    assert!(matches!(restore, Outcome::Rejected(_)), "{restore:?}");
    assert!(
        real.join(".local/share/mise/installs/cosign/2.9.0/bin/cosign")
            .is_file()
    );
    assert!(real.join(".rustup/toolchains/nightly/bin/rustc").is_file());
}

#[test]
fn a_home_below_a_symlinked_ancestor_is_still_cleared() {
    let node = node("tools-home-clear-linked-ancestor");
    let real = node.root.join("real-parent");
    let home = real.join("home");
    baked_extras(&home);
    let alias = node.root.join("alias-parent");
    symlink(&real, &alias).unwrap_or_else(|e| unreachable!("{e}"));
    let outcome = go(&node, Verb::Clear, &alias.join("home"));
    assert_eq!(outcome, Outcome::Cleared);
    assert!(!home.join(".local/share/mise").exists());
    assert!(!home.join(".rustup").exists());
}

#[test]
fn a_dangling_or_looping_home_link_is_a_warning_and_deletes_nothing() {
    let node = node("tools-home-clear-dangling-home");
    saved(&node);
    let dangling = node.root.join("dangling-home");
    symlink(node.root.join("missing"), &dangling).unwrap_or_else(|e| unreachable!("{e}"));
    let (first, second) = (node.root.join("loop-a"), node.root.join("loop-b"));
    symlink(&second, &first).unwrap_or_else(|e| unreachable!("{e}"));
    symlink(&first, &second).unwrap_or_else(|e| unreachable!("{e}"));
    for home in [&dangling, &first] {
        for verb in [Verb::Clear, Verb::Restore] {
            let outcome = go(&node, verb, home);
            assert!(
                matches!(&outcome, Outcome::Rejected(reason) if reason.starts_with("HOME is")),
                "{verb:?} {}: {outcome:?}",
                home.display()
            );
        }
        assert!(
            fs::symlink_metadata(home).is_ok_and(|metadata| metadata.file_type().is_symlink()),
            "{}",
            home.display()
        );
    }
    assert!(!node.root.join("missing").exists());
}

#[test]
fn a_dangling_home_link_with_a_trailing_slash_or_no_stored_entry_is_a_warning() {
    let node = node("tools-home-clear-dangling-slash");
    let dangling = node.root.join("dangling-home");
    symlink(node.root.join("missing"), &dangling).unwrap_or_else(|e| unreachable!("{e}"));
    let slashed = PathBuf::from(format!("{}/", dangling.display()));
    for home in [&slashed, &dangling] {
        for verb in [Verb::Clear, Verb::Restore] {
            let outcome = go(&node, verb, home);
            assert!(
                matches!(&outcome, Outcome::Rejected(reason) if reason.starts_with("HOME is")),
                "{verb:?} {}: {outcome:?}",
                home.display()
            );
        }
    }
    assert!(
        fs::symlink_metadata(&dangling).is_ok_and(|metadata| metadata.file_type().is_symlink())
    );
}

fn tools_local(home: &Path, verb: &str, output: &Path) -> std::process::Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_velnor-ci"));
    command
        .env("HOME", home)
        .env("GITHUB_OUTPUT", output)
        .env("GITHUB_EVENT_NAME", "pull_request")
        .env("GITHUB_REF", "refs/pull/1/merge")
        .env_remove("GITHUB_EVENT_PATH")
        .env_remove("RUNNER_TEMP")
        .env_remove("VELNOR_MBX_SHARED_DIR")
        .env_remove("VELNOR_MBX_SEED_DIR")
        .args(["tools-local", verb]);
    if verb != "clear" {
        command.args(["--key", KEY]);
    }
    command.output().unwrap_or_else(|e| unreachable!("{e}"))
}

#[test]
fn a_missing_or_empty_home_is_a_warning_and_restore_still_reports_a_miss() {
    let node = node("tools-home-clear-missing-home");
    for (name, home) in [
        ("missing", node.root.join("missing-home")),
        ("empty", PathBuf::new()),
    ] {
        let cleared = tools_local(&home, "clear", &node.root.join(format!("{name}-clear")));
        assert!(cleared.status.success(), "{name}: {cleared:?}");
        assert!(
            String::from_utf8_lossy(&cleared.stdout).starts_with("::warning::"),
            "{name}: {cleared:?}"
        );
        let output_file = node.root.join(format!("{name}-restore"));
        let restored = tools_local(&home, "restore", &output_file);
        assert!(restored.status.success(), "{name}: {restored:?}");
        assert!(
            String::from_utf8_lossy(&restored.stdout).starts_with("::warning::"),
            "{name}: {restored:?}"
        );
        assert_eq!(
            fs::read_to_string(&output_file).unwrap_or_default(),
            "hit=false\n",
            "{name}"
        );
    }
}

#[test]
fn a_save_through_a_link_to_root_is_refused_for_a_store_the_job_owns() {
    let node = node("tools-home-save-root-owned");
    let linked = node.root.join("root-owned-home");
    symlink("/", &linked).unwrap_or_else(|e| unreachable!("{e}"));
    let outcome = go(&node, Verb::Save, &linked);
    assert_eq!(outcome, Outcome::Rejected("owned by another user"));
    assert!(!node.root.join("store/tools").exists());
}
