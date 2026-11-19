// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsStr;
use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};

use velnor_gen::{GenerateError, expected_with_notes};
use velnor_model::RepoPath;

use crate::facts::TestResult;
use crate::golden::{check_sample, load, samples, update_requested};

struct Copy(PathBuf);

impl Copy {
    fn of(sample: &str, tag: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let target =
            std::env::temp_dir().join(format!("velnor-gen-golden-{tag}-{}", std::process::id()));
        if target.exists() {
            fs::remove_dir_all(&target)?;
        }
        copy(&samples().join(sample), &target)?;
        Ok(Self(target))
    }
}

impl Drop for Copy {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).ok();
    }
}

fn copy(from: &Path, to: &Path) -> TestResult {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

#[test]
fn only_the_value_one_turns_the_golden_update_on() {
    assert!(update_requested(Some(OsStr::new("1"))));
    for off in [None, Some(OsStr::new("0")), Some(OsStr::new(""))] {
        assert!(!update_requested(off), "{off:?}");
    }
}

#[test]
fn a_different_render_is_a_failure_and_leaves_expected_alone_unless_updating() -> TestResult {
    let copy = Copy::of("single-crate", "modes")?;
    let workflow = copy.0.join("expected/.github/workflows/rust-ci.yml");
    let path = RepoPath::new(".github/workflows/rust-ci.yml")?;
    fs::write(&workflow, "stale\n")?;

    let failures = check_sample(&copy.0, update_requested(Some(OsStr::new("0"))))?;
    assert!(
        failures
            .iter()
            .any(|failure| failure.contains(path.as_str())),
        "{failures:?}"
    );
    assert_eq!(fs::read_to_string(&workflow)?, "stale\n");

    check_sample(&copy.0, update_requested(Some(OsStr::new("1"))))?;
    let rendered = expected_with_notes(&load(&copy.0)?)?
        .0
        .get(&path)
        .ok_or("the sample renders no workflow")?
        .clone();
    let written = fs::read_to_string(&workflow)?;
    assert!(
        written == rendered,
        "the expected file was rewritten with other content"
    );
    Ok(())
}

#[test]
fn a_sample_that_records_a_cargo_symlink_is_refused_like_the_live_checkout() -> TestResult {
    let copy = Copy::of("release-bin", "symlink")?;
    assert!(expected_with_notes(&load(&copy.0)?).is_ok());
    fs::write(copy.0.join("symlinks.txt"), ".cargo\n")?;
    assert!(matches!(
        expected_with_notes(&load(&copy.0)?),
        Err(GenerateError::Release(_))
    ));
    Ok(())
}

#[test]
fn a_sample_without_a_symlink_sidecar_is_rejected() -> TestResult {
    let copy = Copy::of("release-bin", "missing-symlink-sidecar")?;
    let symlinks = copy.0.join("symlinks.txt");
    fs::write(&symlinks, "")?;
    fs::remove_file(symlinks)?;
    let error = load(&copy.0)
        .err()
        .ok_or("a sample without a symlink sidecar was accepted")?;
    assert!(error.to_string().contains("symlinks.txt"), "{error}");
    Ok(())
}

#[test]
fn a_symlink_at_a_wanted_file_path_is_refused_like_the_live_checkout() -> TestResult {
    let copy = Copy::of("release-bin", "wanted-symlink")?;
    for path in [
        "rust-toolchain.toml",
        ".velnor/rust-ci.toml",
        ".cargo/config.toml",
        "mise.lock",
        ".github/workflows/ci.yml",
        "Cargo.toml",
    ] {
        fs::write(copy.0.join("symlinks.txt"), format!("{path}\n"))?;
        let error = load(&copy.0)
            .err()
            .ok_or(format!("a symlink at {path} was accepted"))?;
        assert!(error.to_string().contains(path), "{error}");
    }
    Ok(())
}

#[test]
fn a_dangling_symlink_sidecar_is_rejected() -> TestResult {
    let copy = Copy::of("release-bin", "dangling-symlink-sidecar")?;
    let symlinks = copy.0.join("symlinks.txt");
    fs::remove_file(&symlinks)?;
    symlink("missing", &symlinks)?;
    assert!(load(&copy.0).is_err());
    Ok(())
}

#[test]
fn a_non_regular_symlink_sidecar_is_rejected() -> TestResult {
    let copy = Copy::of("release-bin", "non-regular-symlink-sidecar")?;
    let symlinks = copy.0.join("symlinks.txt");
    fs::remove_file(&symlinks)?;
    fs::create_dir(&symlinks)?;
    assert!(load(&copy.0).is_err());
    Ok(())
}
