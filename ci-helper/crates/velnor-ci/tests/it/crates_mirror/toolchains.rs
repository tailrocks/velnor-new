// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::io::Repo;

use super::fixture::{TestResult, mirror_fixture};

#[test]
fn hardlinked_rustup_cargo_fetches_uncached_crates() -> TestResult {
    mirror_fixture(|repo, wrapper, config| {
        rustup_proxy(repo, true)?;
        super::direct_fetches_using(repo, wrapper, config, "proxy-bin")
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
        super::child_fetches_using(repo, wrapper, config, "proxy-bin", Some(&home))
    })
}

#[test]
fn directory_override_selection_reaches_cargo_children_in_tmp() -> TestResult {
    mirror_fixture(|repo, wrapper, config| {
        let home = private_toolchains(repo)?;
        fs::create_dir_all(repo.root().join("outer"))?;
        let output = rustup_command(repo, &home)
            .args(["override", "set", "mirror-selected", "--path"])
            .arg(repo.root().join("outer"))
            .output()?;
        assert!(output.status.success(), "{output:?}");
        super::child_fetches_using(repo, wrapper, config, "proxy-bin", Some(&home))
    })
}

fn rustup_proxy(repo: &Repo, hardlink: bool) -> TestResult {
    let output = Command::new("mise").args(["which", "rustup"]).output()?;
    assert!(output.status.success(), "{output:?}");
    let installed = fs::canonicalize(String::from_utf8(output.stdout)?.trim())?;
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
        .env("RUSTUP_HOME", home)
        .env_remove("RUSTUP_TOOLCHAIN");
    command
}
