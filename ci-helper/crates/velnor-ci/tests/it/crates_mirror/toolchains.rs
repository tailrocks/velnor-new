// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::os::unix::fs::MetadataExt;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::io::Repo;

use super::fixture::{TestResult, mirror_fixture, mirror_fixture_named, rustup_binary};

#[test]
fn hardlinked_rustup_cargo_fetches_uncached_crates() -> TestResult {
    mirror_fixture(|repo, wrapper, config| {
        rustup_proxy(repo, true)?;
        super::direct_fetches_using(repo, wrapper, config, "proxy-bin")
    })
}

#[test]
fn nested_explicit_toolchain_survives_changed_directory_and_removed_rustup_path() -> TestResult {
    mirror_fixture(|repo, wrapper, config| {
        let home = private_toolchains(repo)?;
        let root = repo.root();
        fs::remove_file(root.join("proxy-bin/cargo"))?;
        let payload = root.join("proxy-bin/tool-dispatcher");
        fs::rename(root.join("proxy-bin/rustup"), &payload)?;
        std::os::unix::fs::symlink(&payload, root.join("proxy-bin/rustup"))?;
        fs::hard_link(&payload, root.join("proxy-bin/cargo"))?;
        let output = rustup_command(repo, &home)
            .args(["which", "rustc"])
            .output()?;
        assert!(output.status.success(), "{output:?}");
        let rustc = String::from_utf8(output.stdout)?;
        repo.write_unstaged("project/Cargo.toml", "[package]\nname='consumer'\nversion='1.0.0'\nedition='2024'\n[dependencies]\nprobe='=1.0.0'\n");
        repo.write_unstaged("project/src/lib.rs", "");
        fs::create_dir(root.join("tmp"))?;
        repo.write_unstaged(
            "commands/cargo-switch",
            r#"#!/bin/sh
set -eu
test "$RUSTUP_TOOLCHAIN" = "$RUSTUP_HOME/toolchains/mirror-default"
cd "$MIRROR_FIXTURE_ROOT/tmp"
export PATH="$MIRROR_CHILD_PATH"
"$CARGO" +mirror-selected verify-selection
"#,
        );
        repo.write_unstaged("commands/cargo-verify-selection", r#"#!/bin/sh
set -eu
test "$RUSTUP_TOOLCHAIN" = mirror-selected
for name in first second; do
    CARGO_HOME="$MIRROR_FIXTURE_ROOT/child-home-$name" "$CARGO" fetch --manifest-path "$MIRROR_FIXTURE_ROOT/project/Cargo.toml"
done
"#);
        for script in ["cargo-switch", "cargo-verify-selection"] {
            fs::set_permissions(
                root.join("commands").join(script),
                fs::Permissions::from_mode(0o755),
            )?;
        }
        let child_path = std::env::join_paths([
            wrapper.parent().ok_or("wrapper parent")?.to_path_buf(),
            root.join("commands"),
        ])?;
        let output = Command::new(wrapper)
            .arg("switch")
            .current_dir(root.join("project"))
            .env_remove("RUSTUP_TOOLCHAIN")
            .env_remove("VELNOR_CARGO_REAL")
            .env_remove("VELNOR_CARGO_RUSTUP")
            .env("RUSTUP_HOME", &home)
            .env("RUSTC", rustc.trim())
            .env("CARGO_HOME", root.join("outer-home"))
            .env("VELNOR_CARGO_CONFIG", config)
            .env("VELNOR_CARGO_WRAPPER", wrapper)
            .env("MIRROR_FIXTURE_ROOT", root)
            .env("MIRROR_CHILD_PATH", child_path)
            .env(
                "PATH",
                std::env::join_paths([PathBuf::from("../proxy-bin"), root.join("commands")])?,
            )
            .env("CARGO_NET_OFFLINE", "false")
            .env("CARGO_HTTP_PROXY", "http://127.0.0.1:1")
            .env("NO_PROXY", "127.0.0.1")
            .env("CARGO_NET_RETRY", "0")
            .env(
                "CARGO_REGISTRIES_CRATES_IO_INDEX",
                "sparse+http://127.0.0.1:1/index/",
            )
            .output()?;
        assert!(output.status.success(), "{output:?}");
        Ok(())
    })
}

#[test]
fn toolchain_file_selection_reaches_cargo_children_in_tmp() -> TestResult {
    mirror_fixture(|repo, wrapper, config| {
        let home = private_toolchains(repo)?;
        repo.write_unstaged(
            "rust-toolchain.toml",
            "[toolchain]\nchannel='mirror-selected'\n",
        );
        super::child_fetches_using(
            repo,
            wrapper,
            config,
            "proxy-bin",
            Some((&home, "mirror-selected")),
        )
    })
}

#[test]
fn directory_override_selection_reaches_cargo_children_in_tmp() -> TestResult {
    mirror_fixture(directory_override_fetches)
}

#[test]
fn directory_override_with_parentheses_reaches_cargo_children_in_tmp() -> TestResult {
    mirror_fixture_named("crates-mirror (override)", directory_override_fetches)
}

fn directory_override_fetches(repo: &Repo, wrapper: &Path, config: &Path) -> TestResult {
    let home = private_toolchains(repo)?;
    fs::create_dir_all(repo.root().join("outer"))?;
    let output = rustup_command(repo, &home)
        .args(["override", "set", "mirror-selected", "--path"])
        .arg(repo.root().join("outer"))
        .output()?;
    assert!(output.status.success(), "{output:?}");
    super::child_fetches_using(
        repo,
        wrapper,
        config,
        "proxy-bin",
        Some((&home, "mirror-selected")),
    )
}

#[test]
fn path_toolchain_with_spaces_reaches_cargo_children_in_tmp() -> TestResult {
    path_toolchain_fetches("custom toolchain")
}

#[test]
fn path_toolchain_with_parentheses_reaches_cargo_children_in_tmp() -> TestResult {
    path_toolchain_fetches("custom (selected) toolchain")
}

fn path_toolchain_fetches(name: &str) -> TestResult {
    mirror_fixture(|repo, wrapper, config| {
        let home = private_toolchains(repo)?;
        let output = rustup_command(repo, &home)
            .args(["which", "cargo"])
            .output()?;
        assert!(output.status.success(), "{output:?}");
        let cargo = PathBuf::from(String::from_utf8(output.stdout)?.trim());
        let toolchain = cargo.parent().and_then(Path::parent).ok_or("toolchain")?;
        let selected = repo.root().join(name);
        std::os::unix::fs::symlink(toolchain, &selected)?;
        repo.write_unstaged(
            "rust-toolchain.toml",
            &format!("[toolchain]\npath={}\n", serde_json::to_string(&selected)?),
        );
        super::child_fetches_using(
            repo,
            wrapper,
            config,
            "proxy-bin",
            Some((&home, selected.to_str().ok_or("toolchain path")?)),
        )
    })
}

fn rustup_proxy(repo: &Repo, hardlink: bool) -> TestResult {
    let installed = rustup_binary().ok_or("rustup unavailable")?;
    let bin = repo.root().join("proxy-bin");
    fs::create_dir(&bin)?;
    let rustup = bin.join("rustup");
    fs::copy(installed, &rustup)?;
    let cargo = bin.join("cargo");
    if hardlink {
        fs::hard_link(&rustup, &cargo)?;
        let original = fs::metadata(&rustup)?;
        let proxy = fs::symlink_metadata(&cargo)?;
        assert!(proxy.file_type().is_file());
        assert_eq!((proxy.dev(), proxy.ino()), (original.dev(), original.ino()));
    } else {
        std::os::unix::fs::symlink(rustup, cargo)?;
    }
    Ok(())
}

fn private_toolchains(repo: &Repo) -> Result<PathBuf, Box<dyn std::error::Error>> {
    rustup_proxy(repo, false)?;
    let output = Command::new(repo.root().join("proxy-bin/rustup"))
        .args(["which", "cargo"])
        .output()?;
    assert!(output.status.success(), "{output:?}");
    let cargo = PathBuf::from(String::from_utf8(output.stdout)?.trim());
    let toolchain = cargo.parent().and_then(Path::parent).ok_or("toolchain")?;
    let home = repo.root().join("rustup-home");
    for alias in ["mirror-default", "mirror-selected"] {
        let output = rustup_command(repo, &home)
            .args(["toolchain", "link", alias])
            .arg(toolchain)
            .output()?;
        assert!(output.status.success(), "{output:?}");
    }
    let output = rustup_command(repo, &home)
        .args(["default", "mirror-default"])
        .output()?;
    assert!(output.status.success(), "{output:?}");
    Ok(home)
}

fn rustup_command(repo: &Repo, home: &Path) -> Command {
    let mut command = Command::new(repo.root().join("proxy-bin/rustup"));
    command
        .current_dir(repo.root())
        .env("RUSTUP_HOME", home)
        .env_remove("RUSTUP_TOOLCHAIN");
    command
}
