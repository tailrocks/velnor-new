// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::os::unix::fs::{MetadataExt as _, PermissionsExt};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::Result;
use tar::{Builder, EntryType, Header};

use crate::exec_support::Scratch;
use crate::native_cache_support::{
    Message, Store, overwrite_entry, run, run_as, run_in, saved, tar_member, unlock_entry,
};

const STAMP: &str = "2020-01-02T03:04:05.123456789";

fn stamp() -> SystemTime {
    UNIX_EPOCH + Duration::new(1_577_934_245, 123_456_789)
}

fn save_script(body: &str) -> String {
    format!("mkdir -p data && {body}")
}

#[test]
fn hard_links_come_back_as_one_inode_like_gnu_tar_restores_them() -> Result<()> {
    let store = Store::new()?;
    let script = save_script("printf x > data/a && ln data/a data/b");
    let saving = run("cache-save", &store, &[("script", &script)])?;
    assert!(saving.succeeded(), "{}", saving.ran.stdout);
    let restored = run("cache-restore-exact", &store, &[])?;
    assert!(restored.succeeded(), "{}", restored.ran.stdout);
    let first = fs::metadata(restored.workspace_file("data/a"))?;
    let second = fs::metadata(restored.workspace_file("data/b"))?;
    assert_eq!(first.ino(), second.ino());
    assert_eq!(first.nlink(), 2);
    assert_eq!(fs::read_to_string(restored.workspace_file("data/b"))?, "x");
    Ok(())
}

#[test]
fn a_read_only_directory_is_restored_with_its_saved_mode() -> Result<()> {
    let store = Store::new()?;
    let script = save_script("mkdir data/ro && printf x > data/ro/f && chmod 555 data/ro");
    let saving = run("cache-save", &store, &[("script", &script)])?;
    assert!(saving.succeeded(), "{}", saving.ran.stdout);
    let restored = run("cache-restore-exact", &store, &[])?;
    let directory = restored.workspace_file("data/ro");
    let mode = fs::metadata(&directory)?.permissions().mode() & 0o777;
    let content = fs::read_to_string(directory.join("f"));
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o755))?;
    assert!(restored.succeeded(), "{}", restored.ran.stdout);
    assert_eq!(mode, 0o555);
    assert_eq!(content?, "x");
    Ok(())
}

#[test]
fn restored_modes_apply_the_process_umask_like_tar_for_a_non_root_user() -> Result<()> {
    let store = Store::new()?;
    let script = save_script("printf x > data/open && chmod 777 data/open data");
    let saving = run("cache-save", &store, &[("script", &script)])?;
    assert!(saving.succeeded(), "{}", saving.ran.stdout);
    let masked = Message {
        umask: Some(0o022),
        ..Message::default()
    };
    let restored = run_as(Scratch::new()?, "cache-restore-exact", &store, &[], &masked)?;
    assert!(restored.succeeded(), "{}", restored.ran.stdout);
    let mode = fs::metadata(restored.workspace_file("data/open"))?
        .permissions()
        .mode();
    let root = fs::metadata(restored.workspace_file("data"))?
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o755);
    assert_eq!(root & 0o777, 0o755);
    Ok(())
}

#[test]
fn modification_times_survive_to_the_nanosecond_for_files_and_directories() -> Result<()> {
    let store = Store::new()?;
    let script = save_script(&format!(
        "mkdir data/sub && printf x > data/sub/f && TZ=UTC touch -d {STAMP} data/sub/f data/sub data"
    ));
    let saving = run("cache-save", &store, &[("script", &script)])?;
    assert!(saving.succeeded(), "{}", saving.ran.stdout);
    let restored = run("cache-restore-exact", &store, &[])?;
    assert!(restored.succeeded(), "{}", restored.ran.stdout);
    for path in ["data/sub/f", "data/sub", "data"] {
        let metadata = fs::metadata(restored.workspace_file(path))?;
        assert_eq!(metadata.modified()?, stamp(), "{path}");
    }
    Ok(())
}

#[test]
fn a_symlink_keeps_its_own_modification_time() -> Result<()> {
    let store = Store::new()?;
    let script = save_script(&format!(
        "ln -s target data/link && TZ=UTC touch -h -d {STAMP} data/link"
    ));
    let saving = run("cache-save", &store, &[("script", &script)])?;
    assert!(saving.succeeded(), "{}", saving.ran.stdout);
    let restored = run("cache-restore-exact", &store, &[])?;
    assert!(restored.succeeded(), "{}", restored.ran.stdout);
    let link = fs::symlink_metadata(restored.workspace_file("data/link"))?;
    assert_eq!(link.modified()?, stamp());
    Ok(())
}

#[test]
fn modification_times_before_1970_survive() -> Result<()> {
    let store = Store::new()?;
    let script = save_script(
        "printf x > data/f && TZ=UTC touch -d 1969-12-31T23:59:58.5 data/f && TZ=UTC touch -d 1960-01-01T00:00:00.250 data",
    );
    let saving = run("cache-save", &store, &[("script", &script)])?;
    assert!(saving.succeeded(), "{}", saving.ran.stdout);
    let restored = run("cache-restore-exact", &store, &[])?;
    assert!(restored.succeeded(), "{}", restored.ran.stdout);
    let modified = fs::metadata(restored.workspace_file("data/f"))?.modified()?;
    assert_eq!(modified, UNIX_EPOCH - Duration::new(1, 500_000_000));
    let directory = fs::metadata(restored.workspace_file("data"))?.modified()?;
    assert_eq!(
        directory,
        UNIX_EPOCH - Duration::new(315_619_199, 750_000_000)
    );
    Ok(())
}

#[test]
fn restoring_a_file_leaves_its_access_time_current() -> Result<()> {
    let store = Store::new()?;
    let script = save_script(&format!(
        "printf x > data/f && TZ=UTC touch -d {STAMP} data/f"
    ));
    let saving = run("cache-save", &store, &[("script", &script)])?;
    assert!(saving.succeeded(), "{}", saving.ran.stdout);
    let restored = run("cache-restore-exact", &store, &[])?;
    let accessed = fs::metadata(restored.workspace_file("data/f"))?.accessed()?;
    assert!(
        accessed > SystemTime::now() - Duration::from_secs(3600),
        "{accessed:?}"
    );
    Ok(())
}

#[test]
fn a_fifo_in_a_cached_path_fails_the_save_instead_of_publishing_it() -> Result<()> {
    let store = Store::new()?;
    let script = save_script("printf x > data/f && mkfifo data/pipe");
    let saving = run("cache-save", &store, &[("script", &script)])?;
    assert!(saving.succeeded(), "{}", saving.ran.stdout);
    assert!(
        saving
            .ran
            .stdout
            .contains("::warning::Failed to save: archive "),
        "{}",
        saving.ran.stdout
    );
    assert!(saving.ran.stdout.contains("0/pipe is a Fifo"));
    assert!(store.entries()?.is_empty());
    Ok(())
}

#[test]
fn a_symlink_to_a_directory_is_archived_as_a_link() -> Result<()> {
    let store = Store::new()?;
    let script = save_script(
        "mkdir ../outside && printf s3cr3t > ../outside/s && ln -s ../../outside data/link",
    );
    let saving = run("cache-save", &store, &[("script", &script)])?;
    assert!(saving.succeeded(), "{}", saving.ran.stdout);
    let archive = fs::read(unlock_entry_of(&store)?)?;
    assert!(!archive.windows(6).any(|window| window == b"s3cr3t"));
    let restored = run("cache-restore-exact", &store, &[])?;
    let link = restored.workspace_file("data/link");
    assert!(fs::symlink_metadata(link)?.is_symlink());
    Ok(())
}

fn unlock_entry_of(store: &Store) -> Result<std::path::PathBuf> {
    Ok(store.entry("k-1")?.dir.join("data.tar"))
}

#[test]
fn a_file_in_the_workspace_is_replaced_by_a_cached_directory() -> Result<()> {
    let store = Store::new()?;
    saved(&store, "k-1", "hello")?;
    overwrite_entry(
        &store,
        &[
            tar_member(b"0/sub/", b'5', b"", b""),
            tar_member(b"0/sub/x", b'0', b"", b"cached"),
        ],
    )?;
    let work = Scratch::new()?;
    fs::create_dir_all(work.workspace().join("data"))?;
    fs::write(work.workspace().join("data/sub"), "in the way")?;
    let restored = run_in(work, "cache-restore-exact", &store, &[])?;
    assert!(restored.succeeded(), "{}", restored.ran.stdout);
    assert_eq!(
        fs::read_to_string(restored.workspace_file("data/sub/x"))?,
        "cached"
    );
    Ok(())
}

#[test]
fn a_tree_deeper_than_the_open_file_limit_restores() -> Result<()> {
    const DEPTH: usize = 200;
    let store = Store::new()?;
    saved(&store, "k-1", "hello")?;
    let mut builder = Builder::new(Vec::new());
    let mut path = String::from("0");
    for _ in 0..DEPTH {
        path.push_str("/d");
        let mut header = Header::new_gnu();
        header.set_entry_type(EntryType::Directory);
        header.set_mode(0o755);
        header.set_size(0);
        builder.append_data(&mut header, &path, std::io::empty())?;
    }
    let mut header = Header::new_gnu();
    header.set_mode(0o644);
    header.set_size(4);
    builder.append_data(&mut header, format!("{path}/leaf"), &b"deep"[..])?;
    overwrite_entry(&store, &[builder.into_inner()?])?;
    let limited = Message {
        open_files: Some(64),
        ..Message::default()
    };
    let restored = run_as(
        Scratch::new()?,
        "cache-restore-exact",
        &store,
        &[],
        &limited,
    )?;
    assert!(restored.succeeded(), "{}", restored.ran.stdout);
    let leaf = format!("data{}/leaf", "/d".repeat(DEPTH));
    assert_eq!(fs::read_to_string(restored.workspace_file(&leaf))?, "deep");
    Ok(())
}

#[test]
fn an_unreadable_scope_directory_fails_the_restore_instead_of_missing() -> Result<()> {
    let store = Store::new()?;
    saved(&store, "k-1", "hello")?;
    let entry = store.entry("k-1")?.dir;
    let scope = entry.parent().map_or_default(ToOwned::to_owned);
    fs::set_permissions(&scope, fs::Permissions::from_mode(0o000))?;
    let restored = run("cache-restore-exact", &store, &[]);
    fs::set_permissions(&scope, fs::Permissions::from_mode(0o755))?;
    let restored = restored?;
    assert_eq!(restored.ran.code, Some(1), "{}", restored.ran.stdout);
    assert!(
        !restored.ran.stdout.contains("Cache not found"),
        "{}",
        restored.ran.stdout
    );
    let _ = unlock_entry(&store);
    Ok(())
}

#[test]
fn a_save_under_a_private_umask_still_opens_the_store_to_the_step_uid() -> Result<()> {
    let store = Store::new()?;
    let private = Message {
        umask: Some(0o077),
        ..Message::default()
    };
    let script = save_script("printf x > data/a");
    let saving = run_as(
        Scratch::new()?,
        "cache-save",
        &store,
        &[("script", &script)],
        &private,
    )?;
    assert!(saving.succeeded(), "{}", saving.ran.stdout);
    let entry = store.entry("k-1")?.dir;
    for name in ["key", "version", "accessed", "complete", "data.tar"] {
        let mode = fs::metadata(entry.join(name))?.mode() & 0o7777;
        assert_eq!(mode, 0o444, "{name}");
    }
    assert_eq!(fs::metadata(&entry)?.mode() & 0o7777, 0o555);
    let root = store.root();
    let shared: Vec<_> = entry
        .ancestors()
        .skip(1)
        .take_while(|directory| *directory != root)
        .map(|directory| fs::metadata(directory).map(|meta| meta.mode() & 0o7777))
        .collect::<Result<_, _>>()?;
    assert_eq!(shared, [0o755, 0o711, 0o711, 0o711]);
    assert_eq!(fs::metadata(&root)?.mode() & 0o7777, 0o755);
    Ok(())
}
