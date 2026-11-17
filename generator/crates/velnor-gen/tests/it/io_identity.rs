// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::path::Path;
use std::process::Command;

use crate::release_symlink::{Scratch, git};

type TestResult = Result<(), Box<dyn Error>>;

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
    fs::write(path.join(".velnor/rust-ci.toml"), "full_run = \"weekly\"\n")?;
    fs::write(
        path.join(".velnor/rust-ci.lock"),
        crate::release_fixture::LOCK,
    )?;
    let sample = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples/full-run-weekly/view");
    fs::copy(sample.join("mise.toml"), path.join("mise.toml"))?;
    fs::copy(sample.join("mise.lock"), path.join("mise.lock"))?;
    git(path, &["init", "--quiet"])?;
    git(path, &["add", "--all"])?;
    Ok(())
}

fn schedule(path: &Path) -> Result<String, Box<dyn Error>> {
    let cargo = Path::new(env!("CARGO"));
    let cargo_dir = cargo.parent().ok_or("cargo executable has no parent")?;
    let search_path = std::env::join_paths([cargo_dir, Path::new("/usr/bin"), Path::new("/bin")])?;
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-gen"))
        .args(["--root", ".", "generate"])
        .current_dir(path)
        .env("PATH", search_path)
        .env("MISE_TRUSTED_CONFIG_PATHS", path)
        .output()?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    let workflow = fs::read_to_string(path.join(".github/workflows/rust-ci.yml"))?;
    workflow
        .lines()
        .find_map(|line| line.trim().strip_prefix("- cron: "))
        .map(|line| line.trim_matches('\'').to_owned())
        .ok_or_else(|| format!("schedule missing from workflow:\n{workflow}").into())
}

#[test]
fn schedules_use_the_remote_repository_and_transport_independent_identity() -> TestResult {
    let base = std::env::temp_dir().join(format!("velnor-gen-remotes-{}", std::process::id()));
    fs::remove_dir_all(&base).ok();
    let parent = Scratch(base.clone());
    let first = Scratch(base.join("first"));
    let second = Scratch(base.join("second"));
    let third = Scratch(base.join("third"));
    checkout(&first.0)?;
    checkout(&second.0)?;
    checkout(&third.0)?;
    git(
        &first.0,
        &[
            "remote",
            "add",
            "origin",
            "https://github.com/alice/app.git",
        ],
    )?;
    git(
        &second.0,
        &["remote", "add", "origin", "https://github.com/bob/app.git"],
    )?;
    git(
        &third.0,
        &["remote", "add", "origin", "git@github.com:alice/app.git"],
    )?;
    let alice = schedule(&first.0)?;
    assert_ne!(alice, schedule(&second.0)?);
    assert_eq!(alice, schedule(&third.0)?);
    drop(second);
    drop(third);
    drop(first);
    drop(parent);
    Ok(())
}

#[test]
fn repository_case_does_not_change_schedule_identity() -> TestResult {
    let base = std::env::temp_dir().join(format!("velnor-gen-case-{}", std::process::id()));
    fs::remove_dir_all(&base).ok();
    let parent = Scratch(base.clone());
    let lowercase = Scratch(base.join("lowercase"));
    let mixed_case = Scratch(base.join("mixed-case"));
    checkout(&lowercase.0)?;
    checkout(&mixed_case.0)?;
    git(
        &lowercase.0,
        &[
            "remote",
            "add",
            "origin",
            "https://github.com/alice/app.git",
        ],
    )?;
    git(
        &mixed_case.0,
        &[
            "remote",
            "add",
            "origin",
            "https://github.com/Alice/App.git",
        ],
    )?;
    assert_eq!(schedule(&lowercase.0)?, schedule(&mixed_case.0)?);
    drop(mixed_case);
    drop(lowercase);
    drop(parent);
    Ok(())
}

#[test]
fn a_checkout_without_an_origin_keeps_its_schedule_when_renamed() -> TestResult {
    let base = std::env::temp_dir().join(format!("velnor-gen-no-origin-{}", std::process::id()));
    fs::remove_dir_all(&base).ok();
    let mut repo = Scratch(base.clone());
    checkout(&repo.0)?;
    let before = schedule(&repo.0)?;
    let renamed = base.with_extension("renamed");
    fs::rename(&repo.0, &renamed)?;
    repo.0 = renamed.clone();
    let after = schedule(&renamed)?;
    assert_eq!(before, after);
    drop(repo);
    Ok(())
}

#[test]
fn local_origin_is_rejected_instead_of_writing_a_checkout_dependent_schedule() -> TestResult {
    let base = std::env::temp_dir().join(format!("velnor-gen-local-origin-{}", std::process::id()));
    let repo = Scratch(base);
    checkout(&repo.0)?;
    git(&repo.0, &["remote", "add", "origin", "../mirror/app"])?;
    let cargo = Path::new(env!("CARGO"));
    let cargo_dir = cargo.parent().ok_or("cargo executable has no parent")?;
    let search_path = std::env::join_paths([cargo_dir, Path::new("/usr/bin"), Path::new("/bin")])?;
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-gen"))
        .args(["--root", ".", "generate"])
        .current_dir(&repo.0)
        .env("PATH", search_path)
        .env("MISE_TRUSTED_CONFIG_PATHS", &repo.0)
        .output()?;
    assert!(
        !output.status.success(),
        "local origin unexpectedly generated a workflow"
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("origin"));
    assert!(!repo.0.join(".github/workflows/rust-ci.yml").exists());
    Ok(())
}
