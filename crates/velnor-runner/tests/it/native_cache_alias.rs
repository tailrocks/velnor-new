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

fn attempted_over(
    store: &Store,
    paths: &str,
    prepare: impl FnOnce(&Path, &Path) -> Result<()>,
) -> Result<(Run, PathBuf)> {
    let work = Scratch::new()?;
    let victim = empty_victim(&work)?;
    fs::create_dir_all(work.workspace())?;
    prepare(&work.workspace(), &victim)?;
    let restored = run_in(work, "cache-restore-exact", store, &[("path", paths)])?;
    Ok((restored, victim))
}

fn restored_over(
    store: &Store,
    paths: &str,
    prepare: impl FnOnce(&Path, &Path) -> Result<()>,
) -> Result<(Run, PathBuf)> {
    let (restored, victim) = attempted_over(store, paths, prepare)?;
    assert!(restored.succeeded(), "{}", restored.ran.stdout);
    Ok((restored, victim))
}

fn is_case_insensitive() -> Result<bool> {
    let probe = Scratch::new()?;
    fs::create_dir_all(probe.workspace().join("probe"))?;
    Ok(probe.workspace().join("PROBE").exists())
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
    if !is_case_insensitive()? {
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

const SIBLING_SCRIPT: &str =
    "mkdir -p releases/42/assets && printf x > releases/42/assets/f && ln -s releases/42 current";

fn restored_beside_a_link_to_its_parent(paths: &str) -> Result<()> {
    let store = Store::new()?;
    saved_as(&store, paths, SIBLING_SCRIPT)?;
    let (restored, victim) = restored_over(&store, paths, |workspace, _| {
        fs::create_dir_all(workspace.join("releases/42"))?;
        symlink("releases/42", workspace.join("current"))?;
        Ok(())
    })?;
    assert_empty(&victim)?;
    assert_eq!(
        fs::read_link(restored.workspace_file("current"))?,
        Path::new("releases/42")
    );
    assert_eq!(
        fs::read_to_string(restored.workspace_file("releases/42/assets/f"))?,
        "x"
    );
    Ok(())
}

#[test]
fn a_link_to_the_parent_of_another_declared_path_restores_when_the_link_comes_first() -> Result<()>
{
    restored_beside_a_link_to_its_parent("current\nreleases/42/assets")
}

#[test]
fn a_link_to_the_parent_of_another_declared_path_restores_when_the_link_comes_last() -> Result<()> {
    restored_beside_a_link_to_its_parent("releases/42/assets\ncurrent")
}

fn restored_beside_a_link_to_the_workspace(paths: &str) -> Result<()> {
    let store = Store::new()?;
    saved_as(&store, paths, "mkdir x && printf x > x/f && ln -s . self")?;
    let (restored, victim) = restored_over(&store, paths, |workspace, _| {
        symlink(".", workspace.join("self"))?;
        Ok(())
    })?;
    assert_empty(&victim)?;
    assert_eq!(
        fs::read_link(restored.workspace_file("self"))?,
        Path::new(".")
    );
    assert_eq!(fs::read_to_string(restored.workspace_file("x/f"))?, "x");
    Ok(())
}

#[test]
fn a_link_to_the_workspace_root_does_not_stop_a_sibling_path_from_restoring() -> Result<()> {
    restored_beside_a_link_to_the_workspace("x\nself")
}

#[test]
fn a_link_to_the_workspace_root_does_not_stop_an_earlier_path_from_restoring() -> Result<()> {
    restored_beside_a_link_to_the_workspace("self\nx")
}

const FRESH_SCRIPT: &str =
    "mkdir -p releases/42/assets && printf x > releases/42/assets/f && ln -s releases/42 current";

fn restored_below_a_link_to_a_root_that_is_not_there_yet(paths: &str) -> Result<Run> {
    let store = Store::new()?;
    saved_as(&store, paths, FRESH_SCRIPT)?;
    let (restored, victim) = attempted_over(&store, paths, |workspace, _| {
        symlink("releases/42", workspace.join("current"))?;
        Ok(())
    })?;
    assert_empty(&victim)?;
    assert_eq!(
        fs::read_link(restored.workspace_file("current"))?,
        Path::new("releases/42")
    );
    Ok(restored)
}

#[test]
fn a_path_below_a_link_to_an_earlier_root_that_a_fresh_checkout_lacks_is_restored_below_it()
-> Result<()> {
    let restored =
        restored_below_a_link_to_a_root_that_is_not_there_yet("releases/42\ncurrent/assets")?;
    assert!(restored.succeeded(), "{}", restored.ran.stdout);
    assert_eq!(
        fs::read_to_string(restored.workspace_file("releases/42/assets/f"))?,
        "x"
    );
    Ok(())
}

#[test]
fn a_path_below_a_link_to_a_later_root_that_a_fresh_checkout_lacks_fails_as_gnu_tar_does()
-> Result<()> {
    let restored =
        restored_below_a_link_to_a_root_that_is_not_there_yet("current/assets\nreleases/42")?;
    assert_eq!(restored.ran.code, Some(1), "{}", restored.ran.stdout);
    assert!(
        restored.ran.stdout.contains("never writes through a link"),
        "{}",
        restored.ran.stdout
    );
    assert!(!restored.workspace_file("releases").exists());
    Ok(())
}

#[test]
fn a_path_below_an_alias_of_an_earlier_root_that_a_fresh_checkout_lacks_is_restored_below_it()
-> Result<()> {
    const PATHS: &str = "data\nalias/sub";
    let store = Store::new()?;
    saved_as(
        &store,
        PATHS,
        "mkdir -p data/sub && printf x > data/sub/f && ln -s data alias",
    )?;
    let (restored, victim) = restored_over(&store, PATHS, |workspace, _| {
        symlink("data", workspace.join("alias"))?;
        Ok(())
    })?;
    assert_empty(&victim)?;
    assert_eq!(
        fs::read_link(restored.workspace_file("alias"))?,
        Path::new("data")
    );
    assert_restored_below_the_outer_root(&restored, "sub/f")
}

const DOTDOT_SCRIPT: &str =
    "mkdir -p data data2/sub && printf y > data/g && printf x > data2/sub/f";

fn refused_through_dotdot(link: &str) -> Result<()> {
    const PATHS: &str = "data\ndata2/sub";
    let store = Store::new()?;
    saved_as(&store, PATHS, DOTDOT_SCRIPT)?;
    let (restored, victim) = attempted_over(&store, PATHS, |workspace, victim| {
        fs::create_dir(victim.join("deep"))?;
        symlink("../victim/deep", workspace.join("data"))?;
        symlink(link, workspace.join("data2"))?;
        Ok(())
    })?;
    assert_eq!(restored.ran.code, Some(1), "{}", restored.ran.stdout);
    assert!(
        restored
            .ran
            .stdout
            .contains("leaves another declared path through a link"),
        "{}",
        restored.ran.stdout
    );
    assert_empty(&victim.join("deep"))?;
    assert!(!victim.join("sub").exists());
    Ok(())
}

#[test]
fn a_parent_step_after_the_outer_link_is_refused_instead_of_followed_out() -> Result<()> {
    refused_through_dotdot("data/..")
}

#[test]
fn a_parent_step_after_the_case_alias_of_the_outer_link_is_refused() -> Result<()> {
    if !is_case_insensitive()? {
        return Ok(());
    }
    refused_through_dotdot("DATA/..")
}

#[test]
fn links_into_each_others_directories_are_replaced_by_directories() -> Result<()> {
    const PATHS: &str = "a\nt1/b";
    let store = Store::new()?;
    saved_as(
        &store,
        PATHS,
        "mkdir -p a t1/b && printf x > a/f && printf y > t1/b/g",
    )?;
    let (restored, victim) = restored_over(&store, PATHS, |workspace, _| {
        fs::create_dir(workspace.join("t1"))?;
        symlink("t1", workspace.join("a"))?;
        symlink("..", workspace.join("t1/b"))?;
        Ok(())
    })?;
    assert_empty(&victim)?;
    assert_eq!(fs::read_to_string(restored.workspace_file("a/f"))?, "x");
    assert_eq!(fs::read_to_string(restored.workspace_file("t1/b/g"))?, "y");
    assert!(!fs::symlink_metadata(restored.workspace_file("a"))?.is_symlink());
    Ok(())
}
