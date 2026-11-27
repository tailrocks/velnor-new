// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::exec_support::Scratch;
use crate::native_cache_support::{Run, Store, overwrite_entry, run, run_in, saved, tar_member};

const NESTED: &str = "data\ndata/sub";
const NESTED_SEED: &str = "mkdir -p data/sub && printf hello > data/a.txt";

pub(crate) fn assert_empty(directory: &Path) -> Result<()> {
    let names: Vec<_> = fs::read_dir(directory)?
        .map(|item| item.map(|item| item.file_name()))
        .collect::<Result<_, _>>()?;
    assert_eq!(names, Vec::<std::ffi::OsString>::new());
    Ok(())
}

pub(crate) fn empty_victim(work: &Scratch) -> Result<PathBuf> {
    let victim = work.workspace().with_file_name("victim");
    fs::create_dir_all(&victim)?;
    Ok(victim)
}

fn refused(restored: &crate::native_cache_support::Run) {
    assert_eq!(restored.ran.code, Some(1), "{}", restored.ran.stdout);
    assert!(
        restored
            .ran
            .stdout
            .contains("cache restore never writes through a link"),
        "{}",
        restored.ran.stdout
    );
}

#[test]
fn a_link_restored_at_a_declared_path_closes_it() -> Result<()> {
    let store = Store::new()?;
    saved(&store, "k-1", "hello")?;
    overwrite_entry(
        &store,
        &[
            tar_member(b"0", b'2', b"../victim", b""),
            tar_member(b"0/planted.txt", b'0', b"", b"x"),
        ],
    )?;
    let work = Scratch::new()?;
    let victim = empty_victim(&work)?;
    let restored = run_in(work, "cache-restore-exact", &store, &[])?;
    refused(&restored);
    assert_empty(&victim)?;
    Ok(())
}

#[test]
fn a_link_restored_above_a_nested_declared_path_closes_it() -> Result<()> {
    let store = Store::new()?;
    let saving = run(
        "cache-save",
        &store,
        &[("path", NESTED), ("script", NESTED_SEED)],
    )?;
    assert!(saving.succeeded(), "{}", saving.ran.stdout);
    overwrite_entry(
        &store,
        &[
            tar_member(b"0", b'2', b"../victim", b""),
            tar_member(b"1/planted.txt", b'0', b"", b"x"),
        ],
    )?;
    let work = Scratch::new()?;
    let victim = empty_victim(&work)?;
    let restored = run_in(work, "cache-restore-exact", &store, &[("path", NESTED)])?;
    refused(&restored);
    assert_empty(&victim)?;
    Ok(())
}

#[test]
fn a_pre_existing_alias_does_not_hide_that_a_declared_path_is_nested() -> Result<()> {
    const PATHS: &str = "real\nalias/b/c";
    let store = Store::new()?;
    let saving = run(
        "cache-save",
        &store,
        &[
            ("path", PATHS),
            ("script", "mkdir -p real && printf x > real/a.txt"),
        ],
    )?;
    assert!(saving.succeeded(), "{}", saving.ran.stdout);
    overwrite_entry(
        &store,
        &[
            tar_member(b"0/b", b'2', b"../../victim", b""),
            tar_member(b"1/planted.txt", b'0', b"", b"x"),
        ],
    )?;
    let work = Scratch::new()?;
    let victim = empty_victim(&work)?;
    fs::create_dir_all(work.workspace().join("real"))?;
    symlink("real", work.workspace().join("alias"))?;
    let restored = run_in(work, "cache-restore-exact", &store, &[("path", PATHS)])?;
    refused(&restored);
    assert_empty(&victim)?;
    Ok(())
}

fn restore_over_alias(
    paths: &str,
    prepare: impl FnOnce(&Path) -> Result<()>,
) -> Result<(Run, PathBuf)> {
    let store = Store::new()?;
    let saving = run(
        "cache-save",
        &store,
        &[
            ("path", paths),
            ("script", "mkdir -p real Real && printf x > real/a.txt"),
        ],
    )?;
    assert!(saving.succeeded(), "{}", saving.ran.stdout);
    overwrite_entry(
        &store,
        &[
            tar_member(b"0/", b'5', b"", b""),
            tar_member(b"0/b", b'2', b"../../victim", b""),
            tar_member(b"1/planted.txt", b'0', b"", b"x"),
        ],
    )?;
    let work = Scratch::new()?;
    let victim = empty_victim(&work)?;
    prepare(&work.workspace())?;
    let restored = run_in(work, "cache-restore-exact", &store, &[("path", paths)])?;
    Ok((restored, victim))
}

#[test]
fn a_dangling_alias_in_a_declared_path_never_redirects_a_nested_root() -> Result<()> {
    let (restored, victim) = restore_over_alias("real\nalias/b/c", |workspace| {
        fs::create_dir_all(workspace)?;
        symlink("real", workspace.join("alias"))?;
        Ok(())
    })?;
    assert_empty(&victim)?;
    refused(&restored);
    Ok(())
}

#[test]
fn a_case_only_alias_never_redirects_a_nested_root() -> Result<()> {
    let (restored, victim) = restore_over_alias("Real\nreal/b/c", |_| Ok(()))?;
    assert_empty(&victim)?;
    if restored.succeeded() {
        let planted = restored.workspace_file("real/b/c/planted.txt");
        assert_eq!(fs::read_to_string(planted)?, "x");
    } else {
        refused(&restored);
    }
    Ok(())
}

#[test]
fn a_file_at_a_cached_directory_root_is_replaced_by_the_directory() -> Result<()> {
    let store = Store::new()?;
    saved(&store, "k-1", "hello")?;
    let work = Scratch::new()?;
    fs::create_dir_all(work.workspace())?;
    fs::write(work.workspace().join("data"), "in the way")?;
    let restored = run_in(work, "cache-restore-exact", &store, &[])?;
    assert!(restored.succeeded(), "{}", restored.ran.stdout);
    let data = restored.workspace_file("data");
    assert_eq!(fs::read_to_string(data.join("a.txt"))?, "hello");
    Ok(())
}

#[test]
fn a_declared_path_that_is_a_symlink_is_replaced_by_a_directory() -> Result<()> {
    let store = Store::new()?;
    saved(&store, "k-1", "hello")?;
    let work = Scratch::new()?;
    let outside = work.workspace().join("outside");
    fs::create_dir_all(&outside)?;
    symlink(&outside, work.workspace().join("data"))?;
    let restored = run_in(work, "cache-restore-exact", &store, &[])?;
    assert!(restored.succeeded(), "{}", restored.ran.stdout);
    assert_empty(&outside)?;
    let data = restored.workspace_file("data");
    assert!(!fs::symlink_metadata(&data)?.is_symlink());
    assert_eq!(fs::read_to_string(data.join("a.txt"))?, "hello");
    Ok(())
}

#[test]
fn save_keeps_dangling_symlinks_and_restore_recreates_them() -> Result<()> {
    let store = Store::new()?;
    let script = "mkdir -p data && printf x > data/a.txt && ln -s /nowhere/absolute data/abs && ln -s missing/relative data/rel";
    let saving = run("cache-save", &store, &[("script", script)])?;
    assert!(saving.succeeded(), "{}", saving.ran.stdout);
    assert!(saving.ran.has_line("Cache saved with key: k-1"));
    let restored = run("cache-restore-exact", &store, &[])?;
    assert!(restored.succeeded(), "{}", restored.ran.stdout);
    let data = restored.workspace_file("data");
    assert_eq!(
        fs::read_link(data.join("abs"))?,
        Path::new("/nowhere/absolute")
    );
    assert_eq!(
        fs::read_link(data.join("rel"))?,
        Path::new("missing/relative")
    );
    Ok(())
}

#[test]
fn save_refuses_a_declared_path_below_a_link_that_restore_would_refuse() -> Result<()> {
    const PATHS: &str = "link\nlink/subdir";
    let store = Store::new()?;
    let saving = run(
        "cache-save",
        &store,
        &[
            ("path", PATHS),
            ("script", "mkdir -p real/subdir && ln -s real link"),
        ],
    )?;
    assert!(saving.succeeded(), "{}", saving.ran.stdout);
    assert!(
        saving
            .ran
            .stdout
            .contains("Failed to save: cache restore never writes through a link"),
        "{}",
        saving.ran.stdout
    );
    assert!(!saving.ran.stdout.contains("Cache saved"));
    assert!(store.entries()?.is_empty());
    Ok(())
}
