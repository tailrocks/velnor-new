// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{error::Error, fs, os::unix::fs::PermissionsExt, path::Path, process::Command};

use super::support::{copy_fingerprint_inputs, copy_tree};

pub(super) fn private_directory(path: &Path) -> Result<(), Box<dyn Error>> {
    fs::create_dir(path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

pub(super) fn copy_mutation_repository(
    source: &Path,
    destination: &Path,
) -> Result<(), Box<dyn Error>> {
    copy_fingerprint_inputs(source, destination)?;
    for directory in [".github", ".velnor", "crates", "fixtures", "scripts"] {
        copy_tree(&source.join(directory), &destination.join(directory))?;
    }
    for file in [
        ".gitignore",
        ".mise-version",
        "Cargo.lock",
        "Cargo.toml",
        "mise.lock",
        "mise.toml",
        "rust-toolchain.toml",
    ] {
        let input = source.join(file);
        if input.is_file() {
            fs::copy(&input, destination.join(file))?;
        }
    }
    git(destination, &["init", "-q"])?;
    git(destination, &["add", "-A"])?;
    git(
        destination,
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.hooksPath=/dev/null",
            "commit",
            "-qm",
            "fixture",
        ],
    )?;
    Ok(())
}

fn git(root: &Path, args: &[&str]) -> Result<(), Box<dyn Error>> {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()?;
    if !output.status.success() {
        let error = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Git fixture setup failed: {error}").into());
    }
    Ok(())
}
