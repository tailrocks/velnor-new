// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use crate::private_sources::SECTION;
use crate::release_symlink::{Scratch, git};

type TestResult = Result<(), Box<dyn Error>>;

const WARNING: &str = "velnor-gen: [private_sources] / release assets with helper 0.1.110 mint the App token after repository-controlled steps; upgrade the helper to 0.1.111 or later";

fn checkout(path: &Path) -> TestResult {
    fs::create_dir_all(path.join(".velnor"))?;
    fs::create_dir_all(path.join("src"))?;
    fs::write(
        path.join("Cargo.toml"),
        "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )?;
    fs::write(path.join("src/lib.rs"), "")?;
    fs::write(
        path.join("Cargo.lock"),
        "version = 4\n\n[[package]]\nname = \"fixture\"\nversion = \"0.1.0\"\n",
    )?;
    fs::write(
        path.join(".velnor/rust-ci.toml"),
        format!("full_run = \"weekly\"\n{SECTION}"),
    )?;
    fs::write(
        path.join(".velnor/rust-ci.lock"),
        crate::release_fixture::LOCK.replacen("version = \"0.1.81\"", "version = \"0.1.110\"", 1),
    )?;
    let sample =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../generator/samples/full-run-weekly/view");
    fs::copy(sample.join("mise.toml"), path.join("mise.toml"))?;
    fs::copy(sample.join("mise.lock"), path.join("mise.lock"))?;
    git(path, &["init", "--quiet"])?;
    git(path, &["add", "--all"])?;
    Ok(())
}

fn generate(path: &Path) -> Result<Output, Box<dyn Error>> {
    let cargo = velnor_test_support::real_cargo();
    let cargo_dir = cargo.parent().ok_or("cargo executable has no parent")?;
    let search_path = std::env::join_paths([cargo_dir, Path::new("/usr/bin"), Path::new("/bin")])?;
    Ok(Command::new(env!("CARGO_BIN_EXE_velnor-gen"))
        .args(["--root", ".", "generate"])
        .current_dir(path)
        .env("PATH", search_path)
        .env("MISE_TRUSTED_CONFIG_PATHS", path)
        .output()?)
}

#[test]
fn generate_warns_on_stderr_when_an_older_helper_mints_the_token_late() -> TestResult {
    let base =
        std::env::temp_dir().join(format!("velnor-gen-helper-warning-{}", std::process::id()));
    fs::remove_dir_all(&base).ok();
    let scratch = Scratch(base.clone());
    checkout(&scratch.0)?;
    let output = generate(&scratch.0)?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    assert!(stderr.lines().any(|line| line == WARNING), "{stderr}");
    drop(scratch);
    Ok(())
}
