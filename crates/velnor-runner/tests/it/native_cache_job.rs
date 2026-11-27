// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs::{self, File};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{Result, bail};
use rustix::fs::{FlockOperation, flock};

use crate::exec_support::Scratch;
use crate::native_cache_support::{
    Message, Store, bytes_under, command, nothing_left_in_the_broker_dirs, patched, run, run_as,
    run_in, saved, seed_and_rewrite_event, spawn, tar_member, terminate_and_wait, unlock_entry,
    wait_until,
};

#[test]
fn a_step_rewriting_the_event_file_does_not_unlock_a_fork_save() -> Result<()> {
    let store = Store::new()?;
    let script = seed_and_rewrite_event();
    let fork = run("cache-fork-pull-request", &store, &[("script", &script)])?;
    assert!(fork.succeeded(), "{}", fork.ran.stdout);
    assert!(
        fork.ran
            .has_line("::warning::Failed to save: cache write denied: fork pull request"),
        "{}",
        fork.ran.stdout
    );
    assert!(store.entries()?.is_empty());
    Ok(())
}

#[test]
fn a_fork_save_is_denied_before_its_paths_are_inspected() -> Result<()> {
    let store = Store::new()?;
    let fork = run(
        "cache-fork-pull-request",
        &store,
        &[("script", "mkdir -p data && mkfifo data/pipe")],
    )?;
    assert!(fork.succeeded(), "{}", fork.ran.stdout);
    assert!(
        fork.ran
            .has_line("::warning::Failed to save: cache write denied: fork pull request"),
        "{}",
        fork.ran.stdout
    );
    Ok(())
}

#[test]
fn version_ignores_the_runner_temp_and_workspace_prefixes() -> Result<()> {
    let store = Store::new()?;
    let bases: [fn(&Scratch) -> PathBuf; 2] =
        [|work| work.options().temp, |work| work.options().workspace];
    for (index, base) in bases.into_iter().enumerate() {
        let key = format!("slot-{index}");
        let saving = Scratch::new()?;
        let path = base(&saving).join("t").to_string_lossy().into_owned();
        let script = format!("mkdir -p {path} && printf hello > {path}/a.txt");
        let edits = [("path", path.as_str()), ("key", &key), ("script", &script)];
        let saved = run_in(saving, "cache-save", &store, &edits)?;
        assert!(saved.succeeded(), "{}", saved.ran.stdout);
        assert!(saved.ran.has_line(&format!("Cache saved with key: {key}")));
        let restoring = Scratch::new()?;
        let path = base(&restoring).join("t").to_string_lossy().into_owned();
        let edits = [("path", path.as_str()), ("key", &key)];
        let restored = run_in(restoring, "cache-restore-exact", &store, &edits)?;
        assert!(
            restored
                .ran
                .has_line(&format!("hit=[true] matched=[{key}] primary=[{key}]")),
            "{}",
            restored.ran.stdout
        );
        assert_eq!(fs::read_to_string(format!("{path}/a.txt"))?, "hello");
    }
    Ok(())
}

#[test]
fn interrupted_save_is_never_visible_to_a_restore() -> Result<()> {
    let store = Store::new()?;
    let work = Scratch::new()?;
    let script = "mkdir -p data && head -c 134217728 /dev/zero > data/big";
    let job = patched("cache-save", &work, &[("script", script)])?;
    let mut child = command(&job, &work, &store)
        .stdout(std::process::Stdio::null())
        .spawn()?;
    let started = Instant::now();
    while bytes_under(&store.root()) < (4 << 20) {
        if started.elapsed() > Duration::from_secs(60) || child.try_wait()?.is_some() {
            bail!("the save finished before it wrote into the store");
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    child.kill()?;
    child.wait()?;
    let restored = run("cache-restore-exact", &store, &[])?;
    assert!(restored.succeeded(), "{}", restored.ran.stdout);
    assert!(
        restored.ran.has_line("hit=[] matched=[] primary=[k-1]"),
        "{}",
        restored.ran.stdout
    );
    assert!(!restored.workspace_file("data").exists());
    saved(&store, "k-1", "hello")?;
    let again = run("cache-restore-exact", &store, &[])?;
    assert_eq!(
        fs::read_to_string(again.workspace_file("data/a.txt"))?,
        "hello"
    );
    Ok(())
}

#[test]
fn a_crashed_publish_leftover_is_replaced_by_the_next_save() -> Result<()> {
    let store = Store::new()?;
    saved(&store, "k-1", "old")?;
    fs::remove_file(unlock_entry(&store)?.join("complete"))?;
    let missed = run("cache-restore-exact", &store, &[])?;
    assert!(
        missed.ran.has_line("hit=[] matched=[] primary=[k-1]"),
        "{}",
        missed.ran.stdout
    );
    saved(&store, "k-1", "new")?;
    assert_eq!(store.entries()?.len(), 1);
    nothing_left_in_the_broker_dirs(&store)?;
    let restored = run("cache-restore-exact", &store, &[])?;
    assert_eq!(
        fs::read_to_string(restored.workspace_file("data/a.txt"))?,
        "new"
    );
    Ok(())
}

#[test]
fn a_job_without_a_usable_repository_id_fails_the_cache_step() -> Result<()> {
    let store = Store::new()?;
    saved(&store, "k-1", "hello")?;
    for id in ["0", "", "not-a-number"] {
        let broken = Message {
            github: &[("repository_id", id)],
            ..Message::default()
        };
        let restored = run_as(Scratch::new()?, "cache-restore-exact", &store, &[], &broken)?;
        assert_eq!(restored.ran.code, Some(1), "{id}: {}", restored.ran.stdout);
        assert!(
            restored
                .ran
                .stdout
                .contains("trusted facts: repository_id is missing"),
            "{id}: {}",
            restored.ran.stdout
        );
    }
    Ok(())
}

#[test]
fn another_repository_does_not_see_the_entry() -> Result<()> {
    let store = Store::new()?;
    saved(&store, "k-1", "hello")?;
    let other = Message {
        github: &[("repository_id", "99")],
        ..Message::default()
    };
    let restored = run_as(Scratch::new()?, "cache-restore-exact", &store, &[], &other)?;
    assert!(restored.succeeded(), "{}", restored.ran.stdout);
    assert!(
        restored.ran.has_line("hit=[] matched=[] primary=[k-1]"),
        "{}",
        restored.ran.stdout
    );
    Ok(())
}

#[test]
fn restore_rejects_a_member_name_leaving_the_declared_path() -> Result<()> {
    let store = Store::new()?;
    saved(&store, "k-1", "hello")?;
    let mut archive = tar_member(b"0/../../../escape.txt", b'0', b"", b"x");
    archive.extend_from_slice(&[0; 1024]);
    fs::write(unlock_entry(&store)?.join("data.tar"), archive)?;
    let restored = run("cache-restore-exact", &store, &[])?;
    assert_eq!(restored.ran.code, Some(1), "{}", restored.ran.stdout);
    assert!(
        restored.ran.stdout.contains("escapes the declared paths"),
        "{}",
        restored.ran.stdout
    );
    assert!(fs::symlink_metadata(restored.workspace_file("data/../../../escape.txt")).is_err());
    Ok(())
}

fn frozen_upload(incoming: &std::path::Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    fs::read_dir(incoming)
        .into_iter()
        .flatten()
        .flatten()
        .any(|id| {
            fs::read_dir(id.path())
                .into_iter()
                .flatten()
                .flatten()
                .any(|upload| {
                    fs::metadata(upload.path().join("data.tar"))
                        .is_ok_and(|meta| meta.len() > 0 && meta.mode() & 0o222 == 0)
                })
        })
}

#[test]
fn cancel_during_save_publishes_nothing() -> Result<()> {
    let store = Store::new()?;
    let work = Scratch::new()?;
    let script = "mkdir -p data && head -c 134217728 /dev/zero > data/big";
    let child = spawn(
        "cache-save",
        &work,
        &store,
        &[("script", script)],
        &Message::default(),
    )?;
    let incoming = store.root().join("incoming");
    wait_until("the archive to grow in the incoming directory", || {
        bytes_under(&incoming) > (1 << 20)
    })?;
    let ran = terminate_and_wait(child)?;
    assert_eq!(ran.code, Some(1), "{}", ran.stdout);
    assert!(ran.has_line("step 2 cancelled"), "{}", ran.stdout);
    assert!(ran.has_line("job cancelled"), "{}", ran.stdout);
    assert!(!ran.stdout.contains("Cache saved"), "{}", ran.stdout);
    assert!(store.entries()?.is_empty());
    assert_eq!(bytes_under(&incoming), 0);
    Ok(())
}

#[test]
fn cancel_while_the_publish_waits_for_the_scope_lock_stops_the_job() -> Result<()> {
    let store = Store::new()?;
    saved(&store, "k-0", "first")?;
    let held = fs::read_dir(store.root().join("locks"))?
        .map(|item| {
            let file = File::open(item?.path())?;
            flock(&file, FlockOperation::NonBlockingLockExclusive)?;
            Ok(file)
        })
        .collect::<Result<Vec<_>>>()?;
    assert!(!held.is_empty());
    let work = Scratch::new()?;
    let child = spawn(
        "cache-save",
        &work,
        &store,
        &[("key", "k-1")],
        &Message::default(),
    )?;
    let incoming = store.root().join("incoming");
    wait_until("the frozen upload at the scope lock", || {
        frozen_upload(&incoming)
    })?;
    let started = Instant::now();
    let ran = terminate_and_wait(child)?;
    assert!(started.elapsed() < Duration::from_secs(10));
    assert!(ran.has_line("step 2 cancelled"), "{}", ran.stdout);
    assert!(!ran.stdout.contains("Cache saved"), "{}", ran.stdout);
    assert_eq!(store.entries()?.len(), 1);
    assert_eq!(bytes_under(&incoming), 0);
    drop(held);
    Ok(())
}

#[test]
fn cancel_during_restore_stops_between_members() -> Result<()> {
    const MEMBERS: usize = 20_000;
    let store = Store::new()?;
    saved(&store, "k-1", "hello")?;
    let mut archive = Vec::new();
    for index in 0..MEMBERS {
        let name = format!("0/f{index:05}");
        archive.extend(tar_member(name.as_bytes(), b'0', b"", b"x"));
    }
    archive.extend_from_slice(&[0; 1024]);
    fs::write(unlock_entry(&store)?.join("data.tar"), archive)?;
    let work = Scratch::new()?;
    let child = spawn(
        "cache-restore-exact",
        &work,
        &store,
        &[],
        &Message::default(),
    )?;
    let first = work.workspace().join("data/f00000");
    wait_until("the restore to start", || first.exists())?;
    let ran = terminate_and_wait(child)?;
    assert_eq!(ran.code, Some(1), "{}", ran.stdout);
    assert!(ran.has_line("step 1 cancelled"), "{}", ran.stdout);
    assert!(ran.has_line("job cancelled"), "{}", ran.stdout);
    let restored = || fs::read_dir(work.workspace().join("data")).map(Iterator::count);
    let settled = restored()?;
    assert!(settled < MEMBERS, "{settled}");
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(restored()?, settled);
    Ok(())
}
