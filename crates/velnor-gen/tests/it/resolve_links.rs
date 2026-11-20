// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};

use velnor_rust::resolve;

type TestResult = Result<(), Box<dyn Error>>;

/// A fresh directory under the temp dir, canonical so its paths compare equal to resolved ones.
fn scratch(tag: &str) -> Result<PathBuf, Box<dyn Error>> {
    let root = std::env::temp_dir().join(format!("velnor-resolve-{tag}-{}", std::process::id()));
    fs::remove_dir_all(&root).ok();
    fs::create_dir_all(&root)?;
    Ok(fs::canonicalize(&root)?)
}

#[test]
fn dangling_symlink_resolves_to_its_target_not_its_own_path() -> TestResult {
    let d = scratch("dangling")?;
    fs::create_dir_all(d.join("repo"))?;
    symlink(d.join("elsewhere"), d.join("repo/target"))?;
    assert_eq!(resolve(&d.join("repo/target"))?, d.join("elsewhere"));
    Ok(())
}

#[test]
fn symlink_loop_is_an_error_not_a_missing_path() -> TestResult {
    let d = scratch("loop")?;
    symlink(d.join("a"), d.join("b"))?;
    symlink(d.join("b"), d.join("a"))?;
    assert!(resolve(&d.join("a")).is_err());
    Ok(())
}

#[test]
fn dotdot_after_symlink_in_existing_prefix_follows_the_os() -> TestResult {
    let d = scratch("dotdot")?;
    fs::create_dir_all(d.join("outside/deep"))?;
    fs::create_dir_all(d.join("repo"))?;
    symlink(d.join("outside/deep"), d.join("repo/link"))?;
    let os = fs::canonicalize(d.join("repo/link/.."))?.join("new");
    assert_eq!(resolve(&d.join("repo/link/../new"))?, os);
    Ok(())
}

#[test]
fn relative_inputs_are_made_absolute() -> TestResult {
    let cwd = fs::canonicalize(std::env::current_dir()?)?;
    assert_eq!(resolve(Path::new("."))?, cwd);
    assert_eq!(
        resolve(Path::new("velnor-resolve-missing"))?,
        cwd.join("velnor-resolve-missing")
    );
    Ok(())
}

#[test]
fn missing_paths_keep_their_lexical_tail() -> TestResult {
    let d = scratch("missing")?;
    fs::create_dir_all(d.join("present"))?;
    assert_eq!(
        resolve(&d.join("present/not-yet/deeper"))?,
        d.join("present/not-yet/deeper")
    );
    Ok(())
}
