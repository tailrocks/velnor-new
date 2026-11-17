// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::io::Repo;

use super::fixture::{TestResult, mirror_fixture, rustup_binary};

mod support;
use support::Mise;

#[test]
fn configured_mise_cargo_preserves_activation_and_runs_wrapper_once() -> TestResult {
    mirror_fixture(|repo, wrapper, config| {
        let mise = Mise::new(repo)?;
        mise.configure(repo, "explicit");
        repo.write_unstaged("tmp-0/Cargo.toml", "[package]\nname='consumer'\nversion='1.0.0'\nedition='2024'\n[dependencies]\nprobe='=1.0.0'\n");
        repo.write_unstaged("tmp-0/src/lib.rs", "");
        mise.nested_fetches(repo, wrapper, config, false)?;
        assert_eq!(
            fs::read_to_string(repo.root().join("wrapper-calls"))?,
            "cargo\n"
        );
        Ok(())
    })
}

#[test]
fn externally_activated_mise_wrapper_fetches_from_mirror_through_cargo_env() -> TestResult {
    mirror_fixture(|repo, wrapper, config| {
        let mise = Mise::new(repo)?;
        mise.configure(repo, "explicit");
        repo.write_unstaged("tmp-0/Cargo.toml", "[package]\nname='consumer'\nversion='1.0.0'\nedition='2024'\n[dependencies]\nprobe='=1.0.0'\n");
        repo.write_unstaged("tmp-0/src/lib.rs", "");
        mise.nested_fetches(repo, wrapper, config, true)?;
        assert_eq!(
            fs::read_to_string(repo.root().join("wrapper-calls"))?,
            "cargo\n"
        );
        Ok(())
    })
}

#[test]
fn plain_mise_ignores_stale_cargo_wrappers_and_keeps_other_commands() -> TestResult {
    let repo = Repo::empty("mise-no-cargo-wrapper");
    let mise = Mise::new(&repo)?;
    mise.stale_wrapper(&repo)?;
    mise.configure(&repo, "other");
    mise.dispatch(&repo, &["fixture".to_owned()])?;
    assert_eq!(
        fs::read_to_string(repo.root().join("other-called"))?,
        "called"
    );
    assert!(!repo.root().join("wrapper-calls").exists());
    Ok(())
}

#[test]
fn implicit_mise_boxington_reaches_native_cargo_with_absolute_toolchain() -> TestResult {
    let repo = Repo::empty("mise-implicit-wrapper");
    let mise = Mise::new(&repo)?;
    mise.configure(&repo, "implicit");
    let selected = mise.absolute_toolchain(&repo)?;
    let output = mise.dispatch(
        &repo,
        &[format!("+{}", selected.display()), "--version".to_owned()],
    )?;
    assert!(String::from_utf8(output.stdout)?.starts_with("cargo "));
    assert_eq!(
        fs::read_to_string(repo.root().join("wrapper-calls"))?,
        "cargo\n"
    );
    Ok(())
}

#[test]
fn plain_mise_activation_does_not_require_a_cargo_wrapper() -> TestResult {
    let repo = Repo::empty("mise-plain-activation");
    let mise = Mise::new(&repo)?;
    mise.configure(&repo, "plain");
    mise.dispatch(&repo, &["activation".to_owned()])?;
    assert_eq!(
        fs::read_to_string(repo.root().join("native-activation"))?,
        "activated"
    );
    assert!(!repo.root().join("wrapper-calls").exists());
    Ok(())
}

#[test]
fn mise_reshim_keeps_cargo_interception_and_original_dispatcher() -> TestResult {
    let repo = Repo::empty("mise-repeated-dispatch");
    let mise = Mise::new(&repo)?;
    mise.configure(&repo, "explicit");
    mise.stale_wrapper(&repo)?;
    let original = repo.root().join("mise-data/command-wrappers/bin/cargo");
    let bytes = fs::read(&original)?;
    mise.activate(&repo)?;
    mise.dispatch(&repo, &["--version".to_owned()])?;
    assert_eq!(
        fs::read_to_string(repo.root().join("wrapper-calls"))?,
        "cargo\n"
    );
    assert_eq!(fs::read(&original)?, bytes);
    Ok(())
}

#[test]
fn missing_mise_fails_integration_prerequisite() -> TestResult {
    let repo = Repo::empty("mise-missing-prerequisite");
    fs::create_dir(repo.root().join("empty-path"))?;
    let output = Command::new(std::env::current_exe()?)
        .args([
            "--exact",
            "crates_mirror::mise::plain_mise_activation_does_not_require_a_cargo_wrapper",
            "--nocapture",
        ])
        .env("PATH", repo.root().join("empty-path"))
        .env("HOME", repo.root())
        .output()?;
    assert!(!output.status.success(), "{output:?}");
    assert!(
        String::from_utf8(output.stderr)?.contains("Mise unavailable"),
        "fixture must report its missing Mise prerequisite"
    );
    Ok(())
}
