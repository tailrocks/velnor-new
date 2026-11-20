// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::Path;
use std::process::{Command, Output};

use crate::io::Repo;

const ARTIFACT: &str = "debug/deps/libapp.rlib";

pub(super) fn mbx_stub() -> Repo {
    let stub = Repo::empty("link-roots-mbx");
    stub.write_unstaged(
        "bin/mbx",
        "#!/bin/sh\ncase \"$3\" in\n  target.root) printf '%s' \"$FAKE_MBX_TARGET_ROOT\" ;;\n  cache_dir) printf '%s' \"$FAKE_MBX_CACHE_DIR\" ;;\nesac\n",
    );
    let script = stub.root().join("bin/mbx");
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755))
        .unwrap_or_else(|e| unreachable!("{e}"));
    stub
}

pub(super) fn stub_path(stub: &Repo) -> OsString {
    let mut path = OsString::from(stub.root().join("bin"));
    path.push(":");
    path.push(std::env::var_os("PATH").unwrap_or_default());
    path
}

fn restored(repo: &Repo, stub: &Repo, home: &Path, tune: impl FnOnce(&mut Command)) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_velnor-ci"));
    command
        .args(["restored", "--target-dir", "target"])
        .current_dir(repo.root())
        .env_remove("MBX_TARGET_ROOT")
        .env_remove("MBX_CACHE_DIR")
        .env_remove("XDG_CACHE_HOME")
        .env_remove("FAKE_MBX_TARGET_ROOT")
        .env_remove("FAKE_MBX_CACHE_DIR")
        .env("PATH", stub_path(stub))
        .env("RUNNER_TEMP", repo.root().join("runner-temp"))
        .env("HOME", home);
    tune(&mut command);
    command.output().unwrap_or_else(|e| unreachable!("{e}"))
}

fn lone(name: &str) -> Repo {
    let repo = Repo::new(name);
    repo.write("src/lib.rs", "pub fn one() {}\n");
    repo
}

fn managed(root: &Path, name: &str) -> std::path::PathBuf {
    let dir = root.join(name);
    fs::create_dir_all(dir.join("debug/deps")).unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(dir.join(ARTIFACT), "rlib").unwrap_or_else(|e| unreachable!("{e}"));
    dir
}

#[test]
fn a_target_link_below_the_persistent_absolute_target_root_is_followed() {
    let repo = lone("persistent-absolute");
    let stub = mbx_stub();
    let home = Repo::empty("persistent-absolute-home");
    let roots = Repo::empty("persistent-absolute-roots");
    let dir = managed(roots.root(), "v1/0123456789abcdef");
    symlink(&dir, repo.root().join("target")).unwrap_or_else(|e| unreachable!("{e}"));
    let output = restored(&repo, &stub, home.root(), |command| {
        command.env("FAKE_MBX_TARGET_ROOT", roots.root());
    });
    assert!(
        output.status.success(),
        "{output:?}{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn a_target_link_below_the_persistent_relative_target_root_is_followed() {
    let repo = lone("persistent-relative");
    let stub = mbx_stub();
    let home = Repo::empty("persistent-relative-home");
    let cache = Repo::empty("persistent-relative-cache");
    let dir = managed(&cache.root().join("roots"), "v1/0123456789abcdef");
    symlink(&dir, repo.root().join("target")).unwrap_or_else(|e| unreachable!("{e}"));
    let output = restored(&repo, &stub, home.root(), |command| {
        command.env("MBX_CACHE_DIR", cache.root());
        command.env("FAKE_MBX_TARGET_ROOT", "roots");
    });
    assert!(
        output.status.success(),
        "{output:?}{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn a_target_link_below_the_persistent_cache_dir_is_followed() {
    let repo = lone("persistent-cache-dir");
    let stub = mbx_stub();
    let home = Repo::empty("persistent-cache-dir-home");
    let cache = Repo::empty("persistent-cache-dir-cache");
    let dir = managed(&cache.root().join("targets"), "v1/0123456789abcdef");
    symlink(&dir, repo.root().join("target")).unwrap_or_else(|e| unreachable!("{e}"));
    let output = restored(&repo, &stub, home.root(), |command| {
        command.env("FAKE_MBX_CACHE_DIR", cache.root());
    });
    assert!(
        output.status.success(),
        "{output:?}{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn a_relative_target_root_resolves_below_the_platform_default_cache_dir() {
    let repo = lone("default-cache-relative");
    let stub = mbx_stub();
    let home = Repo::empty("default-cache-relative-home");
    let default = if cfg!(target_os = "macos") {
        "Library/Caches/mbx"
    } else {
        ".cache/mbx"
    };
    let dir = managed(
        &home.root().join(default).join("roots"),
        "v1/0123456789abcdef",
    );
    symlink(&dir, repo.root().join("target")).unwrap_or_else(|e| unreachable!("{e}"));
    let output = restored(&repo, &stub, home.root(), |command| {
        command.env("MBX_TARGET_ROOT", "roots");
    });
    assert!(
        output.status.success(),
        "{output:?}{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn a_target_link_elsewhere_is_still_refused_with_a_persistent_target_root() {
    let repo = lone("persistent-elsewhere");
    let stub = mbx_stub();
    let home = Repo::empty("persistent-elsewhere-home");
    let roots = Repo::empty("persistent-elsewhere-roots");
    managed(roots.root(), "v1/0123456789abcdef");
    let other = Repo::empty("persistent-elsewhere-other");
    let dir = managed(other.root(), "v1/0123456789abcdef");
    symlink(&dir, repo.root().join("target")).unwrap_or_else(|e| unreachable!("{e}"));
    let output = restored(&repo, &stub, home.root(), |command| {
        command.env("FAKE_MBX_TARGET_ROOT", roots.root());
    });
    assert!(!output.status.success(), "{output:?}");
    assert!(dir.join(ARTIFACT).is_file());
}

#[test]
fn saving_skips_a_target_link_with_a_notice_and_leaves_the_linked_directory_alone() {
    let repo = lone("save-link");
    repo.write("Cargo.lock", "version = 4\n");
    let stub = mbx_stub();
    let dir = managed(repo.root(), "cache-real");
    fs::create_dir_all(dir.join("debug/incremental/app")).unwrap_or_else(|e| unreachable!("{e}"));
    symlink("cache-real", repo.root().join("target")).unwrap_or_else(|e| unreachable!("{e}"));
    repo.write_unstaged("temp/velnor-run.log", "   Compiling app v0.1.0 (/app)\n");
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .args(["cache", "decide", "--target-dir", "target"])
        .env("PATH", stub_path(&stub))
        .env("RUNNER_TEMP", repo.root().join("temp"))
        .env("GITHUB_OUTPUT", repo.root().join("output"))
        .env("CARGO_HOME", repo.root().join("cargo-home"))
        .current_dir(repo.root())
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(output.status.success(), "{output:?}");
    let written = fs::read_to_string(repo.root().join("output")).unwrap_or_default();
    assert!(written.contains("save=false\n"), "{written}");
    let report = String::from_utf8_lossy(&output.stdout);
    assert!(
        report.contains("::notice::") && report.contains("link"),
        "{report}"
    );
    assert!(dir.join("debug/incremental/app").is_dir());
    assert!(dir.join(ARTIFACT).is_file());
}
