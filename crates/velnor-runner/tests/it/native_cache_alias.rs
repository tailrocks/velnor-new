// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::exec_support::Scratch;
use crate::native_cache_links::{assert_empty, empty_victim};
use crate::native_cache_support::{Run, Store, run, run_in};

fn saved_as(store: &Store, paths: &str, script: &str) -> Result<()> {
    let saving = run("cache-save", store, &[("path", paths), ("script", script)])?;
    assert!(saving.succeeded(), "{}", saving.ran.stdout);
    assert!(saving.ran.has_line("Cache saved with key: k-1"));
    Ok(())
}

fn restored_over(
    store: &Store,
    paths: &str,
    prepare: impl FnOnce(&Path, &Path) -> Result<()>,
) -> Result<(Run, PathBuf)> {
    let work = Scratch::new()?;
    let victim = empty_victim(&work)?;
    fs::create_dir_all(work.workspace())?;
    prepare(&work.workspace(), &victim)?;
    let restored = run_in(work, "cache-restore-exact", store, &[("path", paths)])?;
    assert!(restored.succeeded(), "{}", restored.ran.stdout);
    Ok((restored, victim))
}

fn assert_restored_below_the_outer_root(restored: &Run, inner: &str) -> Result<()> {
    let data = restored.workspace_file("data");
    assert!(fs::symlink_metadata(&data)?.is_dir());
    assert_eq!(fs::read_to_string(data.join(inner))?, "x");
    Ok(())
}

#[test]
fn a_declared_path_below_an_alias_of_another_declared_path_is_restored_below_it() -> Result<()> {
    const PATHS: &str = "data\nalias/sub";
    let store = Store::new()?;
    saved_as(
        &store,
        PATHS,
        "mkdir -p data/sub && printf x > data/sub/f && ln -s data alias",
    )?;
    let (restored, victim) = restored_over(&store, PATHS, |workspace, _| {
        symlink("../victim", workspace.join("data"))?;
        symlink("data", workspace.join("alias"))?;
        Ok(())
    })?;
    assert_empty(&victim)?;
    assert_restored_below_the_outer_root(&restored, "sub/f")
}

#[test]
fn an_alias_that_points_below_the_outer_root_keeps_that_distance() -> Result<()> {
    const PATHS: &str = "data\nalias/sub";
    let store = Store::new()?;
    saved_as(
        &store,
        PATHS,
        "mkdir -p data/mid/sub && printf x > data/mid/sub/f && ln -s data/mid alias",
    )?;
    let (restored, victim) = restored_over(&store, PATHS, |workspace, victim| {
        fs::create_dir(victim.join("mid"))?;
        symlink("../victim", workspace.join("data"))?;
        symlink("data/mid", workspace.join("alias"))?;
        Ok(())
    })?;
    assert_empty(&victim.join("mid"))?;
    assert!(!restored.workspace_file("data/sub").exists());
    assert_restored_below_the_outer_root(&restored, "mid/sub/f")
}

#[test]
fn a_declared_path_below_its_own_case_alias_is_restored_below_it() -> Result<()> {
    const PATHS: &str = "data\nDATA/sub";
    let probe = Scratch::new()?;
    fs::create_dir_all(probe.workspace().join("probe"))?;
    if !probe.workspace().join("PROBE").exists() {
        return Ok(());
    }
    let store = Store::new()?;
    saved_as(&store, PATHS, "mkdir -p data/sub && printf x > data/sub/f")?;
    let (restored, victim) = restored_over(&store, PATHS, |workspace, _| {
        symlink("../victim", workspace.join("data"))?;
        Ok(())
    })?;
    assert_empty(&victim)?;
    assert_restored_below_the_outer_root(&restored, "sub/f")
}
