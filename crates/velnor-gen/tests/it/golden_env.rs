// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use crate::golden::samples;

type TestResult = Result<(), Box<dyn Error>>;

const GOLDEN_TEST: &str = "golden::every_sample_repository_renders_exactly_its_expected_files";
const RULESET_TEST: &str = "ruleset::ruleset_on_the_golden_prints_the_recorded_json";

struct Root(PathBuf);

impl Root {
    fn of(tag: &str) -> Result<Self, Box<dyn Error>> {
        let path = std::env::temp_dir().join(format!(
            "velnor-gen-golden-env-{tag}-{}",
            std::process::id()
        ));
        if path.exists() {
            fs::remove_dir_all(&path)?;
        }
        fs::create_dir_all(&path)?;
        Ok(Self(path))
    }
}

impl Drop for Root {
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

fn run(test: &str, root: &Path, update: &str) -> Result<Output, Box<dyn Error>> {
    let output = Command::new(std::env::current_exe()?)
        .args(["--exact", test])
        .env("UPDATE_GOLDENS", update)
        .env("VELNOR_GOLDEN_SAMPLES_ROOT", root)
        .output()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains(test), "the child ran no test\n{stdout}");
    Ok(output)
}

#[test]
fn the_update_switch_leaves_the_expected_files_alone_when_it_is_zero() -> TestResult {
    let root = Root::of("expected-zero")?;
    copy(
        &samples().join("single-crate"),
        &root.0.join("single-crate"),
    )?;
    let workflow = root
        .0
        .join("single-crate/expected/.github/workflows/rust-ci.yml");
    fs::write(&workflow, "stale\n")?;
    let output = run(GOLDEN_TEST, &root.0, "0")?;
    assert!(
        !output.status.success(),
        "the stale render was accepted\n{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(fs::read_to_string(&workflow)?, "stale\n");
    Ok(())
}

#[test]
fn the_update_switch_rewrites_the_expected_files_when_it_is_one() -> TestResult {
    let root = Root::of("expected-one")?;
    copy(
        &samples().join("single-crate"),
        &root.0.join("single-crate"),
    )?;
    let workflow = root
        .0
        .join("single-crate/expected/.github/workflows/rust-ci.yml");
    fs::write(&workflow, "stale\n")?;
    let output = run(GOLDEN_TEST, &root.0, "1")?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert_ne!(fs::read_to_string(&workflow)?, "stale\n");
    Ok(())
}

#[test]
fn the_update_switch_leaves_the_ruleset_alone_when_it_is_zero() -> TestResult {
    let root = Root::of("ruleset-zero")?;
    copy(
        &samples().join("release-two-roots"),
        &root.0.join("release-two-roots"),
    )?;
    let ruleset = root.0.join("release-two-roots/ruleset.json");
    fs::write(&ruleset, "stale\n")?;
    let output = run(RULESET_TEST, &root.0, "0")?;
    assert!(
        !output.status.success(),
        "the stale ruleset was accepted\n{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(fs::read_to_string(&ruleset)?, "stale\n");
    Ok(())
}

#[test]
fn the_update_switch_rewrites_the_ruleset_when_it_is_one() -> TestResult {
    let root = Root::of("ruleset-one")?;
    copy(
        &samples().join("release-two-roots"),
        &root.0.join("release-two-roots"),
    )?;
    let ruleset = root.0.join("release-two-roots/ruleset.json");
    fs::write(&ruleset, "stale\n")?;
    let output = run(RULESET_TEST, &root.0, "1")?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert_ne!(fs::read_to_string(&ruleset)?, "stale\n");
    Ok(())
}
