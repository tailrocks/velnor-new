// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use velnor_rust::resolve;
use velnor_test_support::ScratchDir;

type TestResult = Result<(), Box<dyn Error>>;

/// A fresh directory under the temp dir, canonical so its paths compare equal to resolved ones.
fn scratch(tag: &str) -> Result<(ScratchDir, PathBuf), Box<dyn Error>> {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let guard = ScratchDir::new(std::env::temp_dir().join(format!(
        "velnor-resolve-{tag}-{}-{nanos}",
        std::process::id()
    )));
    let canonical = fs::canonicalize(guard.path())?;
    Ok((guard, canonical))
}

#[test]
fn dangling_symlink_resolves_to_its_target_not_its_own_path() -> TestResult {
    let (_guard, d) = scratch("dangling")?;
    fs::create_dir_all(d.join("repo"))?;
    symlink(d.join("elsewhere"), d.join("repo/target"))?;
    assert_eq!(resolve(&d.join("repo/target"))?, d.join("elsewhere"));
    Ok(())
}

#[test]
fn symlink_loop_is_an_error_not_a_missing_path() -> TestResult {
    let (_guard, d) = scratch("loop")?;
    symlink(d.join("a"), d.join("b"))?;
    symlink(d.join("b"), d.join("a"))?;
    assert!(resolve(&d.join("a")).is_err());
    Ok(())
}

#[test]
fn dotdot_after_symlink_in_existing_prefix_follows_the_os() -> TestResult {
    let (_guard, d) = scratch("dotdot")?;
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
    let (_guard, d) = scratch("missing")?;
    fs::create_dir_all(d.join("present"))?;
    assert_eq!(
        resolve(&d.join("present/not-yet/deeper"))?,
        d.join("present/not-yet/deeper")
    );
    Ok(())
}

fn leftovers(tag: &str) -> usize {
    let prefix = format!("velnor-resolve-{tag}-{}", std::process::id());
    fs::read_dir(std::env::temp_dir()).map_or(0, |entries| {
        entries
            .flatten()
            .filter(|entry| entry.file_name().to_string_lossy().starts_with(&prefix))
            .count()
    })
}

fn assert_scratch_leaves_nothing_in_tmpdir(tag: &str) -> TestResult {
    let before = leftovers(tag);
    {
        let (_guard, d) = scratch(tag)?;
        fs::write(d.join("probe"), "")?;
    }
    assert_eq!(leftovers(tag), before);
    Ok(())
}

#[test]
fn a_scratch_directory_leaves_nothing_in_tmpdir_once_its_test_ends() -> TestResult {
    assert_scratch_leaves_nothing_in_tmpdir("selftest")
}

#[test]
fn a_stale_tree_left_by_an_earlier_process_does_not_break_that_check() -> TestResult {
    let stale = ScratchDir::adopt(
        std::env::temp_dir().join(format!("velnor-resolve-stale-{}", std::process::id())),
    );
    fs::create_dir_all(stale.path())?;
    fs::write(stale.join("left"), "")?;
    assert_scratch_leaves_nothing_in_tmpdir("stale")
}
