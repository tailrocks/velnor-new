// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::os::unix::fs::symlink;
use std::path::Path;
use std::process::{Command, Output};

use crate::io::Repo;

const ARTIFACT: &str = "debug/deps/libapp.rlib";

struct Env<'a> {
    cache: &'a Path,
    temp: &'a Path,
    home: &'a Path,
}

fn restored(repo: &Repo, env: &Env<'_>) -> Output {
    let stub = super::io_target_link_roots::mbx_stub();
    let mut command = Command::new(env!("CARGO_BIN_EXE_velnor-ci"));
    command
        .args(["restored", "--target-dir", "target"])
        .current_dir(repo.root())
        .env_remove("MBX_TARGET_ROOT")
        .env_remove("XDG_CACHE_HOME")
        .env("PATH", super::io_target_link_roots::stub_path(&stub))
        .env("MBX_CACHE_DIR", env.cache)
        .env("RUNNER_TEMP", env.temp)
        .env("HOME", env.home);
    command.output().unwrap_or_else(|e| unreachable!("{e}"))
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn store(name: &str) -> (Repo, std::path::PathBuf) {
    let store = Repo::empty(name);
    let managed = store.root().join("targets/v1/0123456789abcdef");
    store.write_unstaged("targets/v1/0123456789abcdef/debug/deps/libapp.rlib", "rlib");
    (store, managed)
}

fn lone(name: &str) -> Repo {
    let repo = Repo::new(name);
    repo.write("src/lib.rs", "pub fn one() {}\n");
    repo
}

fn others() -> (Repo, Repo) {
    (Repo::empty("link-temp"), Repo::empty("link-home"))
}

#[test]
fn a_target_link_into_the_mbx_cache_is_followed() {
    let repo = lone("link-into-cache");
    let (store, managed) = store("link-cache-store");
    let (temp, home) = others();
    symlink(&managed, repo.root().join("target")).unwrap_or_else(|e| unreachable!("{e}"));
    let env = Env {
        cache: store.root(),
        temp: temp.root(),
        home: home.root(),
    };
    let output = restored(&repo, &env);
    assert!(output.status.success(), "{output:?}");
    assert!(managed.join(ARTIFACT).is_file(), "{output:?}");
    assert!(
        fs::symlink_metadata(repo.root().join("target"))
            .is_ok_and(|meta| meta.file_type().is_symlink()),
        "the link was replaced"
    );
}

#[test]
fn a_relative_target_link_into_the_mbx_target_root_is_followed() {
    let repo = lone("link-relative");
    let (store, managed) = store("link-relative-store");
    let (temp, home) = others();
    let up = Path::new("..").join(
        store
            .root()
            .file_name()
            .unwrap_or_else(|| unreachable!("store has a name")),
    );
    symlink(
        up.join("targets/v1/0123456789abcdef"),
        repo.root().join("target"),
    )
    .unwrap_or_else(|e| unreachable!("{e}"));
    let env = Env {
        cache: store.root(),
        temp: temp.root(),
        home: home.root(),
    };
    let output = restored(&repo, &env);
    assert!(output.status.success(), "{output:?}");
    assert!(managed.join(ARTIFACT).is_file());
}

#[test]
fn a_target_link_outside_every_allowed_root_is_refused_and_left_alone() {
    let repo = lone("link-outside");
    let (store, _) = store("link-outside-store");
    let elsewhere = Repo::empty("link-outside-elsewhere");
    elsewhere.write_unstaged(ARTIFACT, "rlib");
    let (temp, home) = others();
    symlink(elsewhere.root(), repo.root().join("target")).unwrap_or_else(|e| unreachable!("{e}"));
    let env = Env {
        cache: store.root(),
        temp: temp.root(),
        home: home.root(),
    };
    let output = restored(&repo, &env);
    assert!(!output.status.success(), "{output:?}");
    let message = stderr(&output);
    assert!(
        message.contains("outside the mbx cache, RUNNER_TEMP and the checkout"),
        "{message}"
    );
    assert!(message.contains("link-outside-elsewhere"), "{message}");
    assert!(elsewhere.root().join(ARTIFACT).is_file());
    assert!(fs::symlink_metadata(repo.root().join("target")).is_ok());
}

#[test]
fn a_target_link_to_the_root_of_an_allowed_directory_is_refused() {
    let repo = lone("link-root");
    let (store, managed) = store("link-root-store");
    let (temp, home) = others();
    symlink(store.root().join("targets"), repo.root().join("target"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let env = Env {
        cache: store.root(),
        temp: temp.root(),
        home: home.root(),
    };
    let output = restored(&repo, &env);
    assert!(!output.status.success(), "{output:?}");
    assert!(stderr(&output).contains("not below"), "{}", stderr(&output));
    assert!(managed.join(ARTIFACT).is_file());
}

#[test]
fn a_target_link_to_tracked_checkout_content_is_refused() {
    let repo = lone("link-tracked");
    let (store, _) = store("link-tracked-store");
    let (temp, home) = others();
    symlink("src", repo.root().join("target")).unwrap_or_else(|e| unreachable!("{e}"));
    let env = Env {
        cache: store.root(),
        temp: temp.root(),
        home: home.root(),
    };
    let output = restored(&repo, &env);
    assert!(!output.status.success(), "{output:?}");
    assert!(repo.root().join("src/lib.rs").is_file(), "{output:?}");
}

#[test]
fn a_target_link_to_an_untracked_directory_in_the_checkout_is_followed() {
    let repo = lone("link-checkout");
    repo.write_unstaged(&format!("cache-real/{ARTIFACT}"), "rlib");
    let (store, _) = store("link-checkout-store");
    let (temp, home) = others();
    symlink("cache-real", repo.root().join("target")).unwrap_or_else(|e| unreachable!("{e}"));
    let env = Env {
        cache: store.root(),
        temp: temp.root(),
        home: home.root(),
    };
    let output = restored(&repo, &env);
    assert!(output.status.success(), "{output:?}");
    assert!(repo.root().join("cache-real").join(ARTIFACT).is_file());
}

#[test]
fn a_target_link_loop_is_refused_with_a_clear_message() {
    let repo = lone("link-loop");
    let (store, _) = store("link-loop-store");
    let (temp, home) = others();
    symlink("loop-back", repo.root().join("target")).unwrap_or_else(|e| unreachable!("{e}"));
    symlink("target", repo.root().join("loop-back")).unwrap_or_else(|e| unreachable!("{e}"));
    let env = Env {
        cache: store.root(),
        temp: temp.root(),
        home: home.root(),
    };
    let output = restored(&repo, &env);
    assert!(!output.status.success(), "{output:?}");
    let message = stderr(&output);
    assert!(message.contains("cannot be resolved"), "{message}");
    assert!(message.contains("target"), "{message}");
}

#[test]
fn a_dangling_target_link_is_refused_with_a_clear_message() {
    let repo = lone("link-dangling");
    let (store, managed) = store("link-dangling-store");
    let (temp, home) = others();
    symlink(managed.join("gone"), repo.root().join("target"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let env = Env {
        cache: store.root(),
        temp: temp.root(),
        home: home.root(),
    };
    let output = restored(&repo, &env);
    assert!(!output.status.success(), "{output:?}");
    assert!(
        stderr(&output).contains("cannot be resolved"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn a_target_link_into_the_shared_store_outside_its_targets_is_refused_and_left_alone() {
    let repo = lone("link-store-actions");
    let (store, _) = store("link-store-actions-store");
    store.write_unstaged("actions/owner/repo/action.yml", "name: shared\n");
    let (temp, home) = others();
    symlink(
        store.root().join("actions/owner"),
        repo.root().join("target"),
    )
    .unwrap_or_else(|e| unreachable!("{e}"));
    let env = Env {
        cache: store.root(),
        temp: temp.root(),
        home: home.root(),
    };
    let output = restored(&repo, &env);
    assert!(!output.status.success(), "{output:?}");
    assert!(
        stderr(&output).contains("refusing to follow"),
        "{}",
        stderr(&output)
    );
    assert!(store.root().join("actions/owner/repo/action.yml").is_file());
    assert!(!store.root().join("actions/owner/.velnor").exists());
}

#[test]
fn a_target_link_to_a_control_directory_is_refused_whatever_its_case() {
    let repo = lone("link-control-case");
    repo.write_unstaged(".Cargo/registry/index", "index");
    let (store, _) = store("link-control-case-store");
    let (temp, home) = others();
    symlink(".Cargo/registry", repo.root().join("target")).unwrap_or_else(|e| unreachable!("{e}"));
    let env = Env {
        cache: store.root(),
        temp: temp.root(),
        home: home.root(),
    };
    let output = restored(&repo, &env);
    assert!(!output.status.success(), "{output:?}");
    assert!(
        stderr(&output).contains("overlaps checkout content"),
        "{}",
        stderr(&output)
    );
    assert!(repo.root().join(".Cargo/registry/index").is_file());
}
