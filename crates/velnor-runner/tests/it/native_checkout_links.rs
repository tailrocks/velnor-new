// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::os::unix::fs::{OpenOptionsExt, symlink};
use std::path::Path;
use std::process::Child;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use rustix::fs::OFlags;

use crate::exec_support::{Ran, Scratch};
use crate::native_checkout::{Rig, finish, git};

const DEADLINE: Duration = Duration::from_secs(30);
const JUNK_DEPTH: usize = 40;

fn warm_mirror_then_advance(rig: &Rig) -> Result<String> {
    let scratch = Scratch::new()?;
    let ran = rig.run("checkout_depth1", &rig.head, &scratch)?;
    assert_eq!(ran.code, Some(0), "{}\n{}", ran.stdout, ran.stderr);
    rig.advance("next")
}

fn lock_path(rig: &Rig) -> std::path::PathBuf {
    let mut path = rig.mirror().into_os_string();
    path.push(".lock");
    path.into()
}

fn replace_with_link(path: &Path, target: &Path) -> Result<()> {
    if path.is_dir() && !path.is_symlink() {
        std::fs::remove_dir_all(path)?;
    } else if path.symlink_metadata().is_ok() {
        std::fs::remove_file(path)?;
    }
    symlink(target, path).with_context(|| format!("link {}", path.display()))
}

fn finish_within(mut child: Child, limit: Duration) -> Result<Ran> {
    let started = Instant::now();
    while child.try_wait()?.is_none() {
        if started.elapsed() > limit {
            child.kill()?;
            child.wait()?;
            bail!("velnor-runner exec did not exit within {limit:?}");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    finish(child)
}

#[test]
fn refs_link_is_replaced_and_its_target_locks_survive() -> Result<()> {
    let rig = Rig::new()?;
    let next = warm_mirror_then_advance(&rig)?;
    let victim = rig.root.join("victim");
    let locks = [victim.join("keep.lock"), victim.join("nested/keep.lock")];
    std::fs::create_dir_all(victim.join("nested"))?;
    for lock in &locks {
        std::fs::write(lock, "")?;
    }
    replace_with_link(&rig.mirror().join("refs"), &victim)?;
    let scratch = Scratch::new()?;
    let ran = rig.run("checkout_depth1", &next, &scratch)?;
    assert_eq!(ran.code, Some(0), "{}\n{}", ran.stdout, ran.stderr);
    assert_eq!(git(&scratch.workspace(), &["rev-parse", "HEAD"])?, next);
    assert!(
        locks.iter().all(|lock| lock.exists()),
        "a target lock was removed"
    );
    assert!(
        !rig.mirror().join("refs").is_symlink(),
        "the refs link was kept"
    );
    Ok(())
}

#[test]
fn lock_file_link_fails_the_fetch_without_touching_the_target() -> Result<()> {
    let rig = Rig::new()?;
    let next = warm_mirror_then_advance(&rig)?;
    let target = rig.root.join("lock-target");
    replace_with_link(&lock_path(&rig), &target)?;
    let scratch = Scratch::new()?;
    let ran = rig.run("checkout_depth1", &next, &scratch)?;
    assert_ne!(ran.code, Some(0), "{}\n{}", ran.stdout, ran.stderr);
    assert!(!target.exists(), "the lock open followed the link");
    Ok(())
}

#[test]
fn head_link_is_replaced_and_its_target_is_not_written() -> Result<()> {
    let rig = Rig::new()?;
    let next = warm_mirror_then_advance(&rig)?;
    let target = rig.root.join("head-target");
    let planted = "ref: refs/heads/victim\n";
    std::fs::write(&target, planted)?;
    replace_with_link(&rig.mirror().join("HEAD"), &target)?;
    let scratch = Scratch::new()?;
    let ran = rig.run("checkout_depth1", &next, &scratch)?;
    assert_eq!(ran.code, Some(0), "{}\n{}", ran.stdout, ran.stderr);
    assert_eq!(std::fs::read_to_string(&target)?, planted);
    assert!(
        rig.mirror().join("HEAD").symlink_metadata()?.is_file(),
        "HEAD was not reset"
    );
    Ok(())
}

#[test]
fn dangling_config_link_is_removed_and_its_target_is_not_created() -> Result<()> {
    let rig = Rig::new()?;
    let next = warm_mirror_then_advance(&rig)?;
    let target = rig.root.join("config-target");
    replace_with_link(&rig.mirror().join("config"), &target)?;
    let scratch = Scratch::new()?;
    let ran = rig.run("checkout_depth1", &next, &scratch)?;
    assert_eq!(ran.code, Some(0), "{}\n{}", ran.stdout, ran.stderr);
    assert!(!target.exists(), "git followed the config link");
    assert!(
        rig.mirror().join("config").symlink_metadata()?.is_file(),
        "config was not reset"
    );
    Ok(())
}

#[test]
fn lock_fifo_fails_the_fetch_naming_the_path_without_hanging() -> Result<()> {
    let rig = Rig::new()?;
    let next = warm_mirror_then_advance(&rig)?;
    let lock = lock_path(&rig);
    std::fs::remove_file(&lock)?;
    let made = std::process::Command::new("mkfifo")
        .arg(&lock)
        .status()
        .context("run mkfifo")?;
    assert!(made.success(), "mkfifo failed");
    let job = rig.job("checkout_depth1", &next, &rig.server_url())?;
    let named = lock.display().to_string();
    let scratch = Scratch::new()?;
    let without_reader = finish_within(rig.start(&job, &scratch)?, DEADLINE)?;
    assert_eq!(without_reader.code, Some(1), "{}", without_reader.stdout);
    assert!(
        without_reader.stdout.contains(&named),
        "{}",
        without_reader.stdout
    );
    let _reader = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(OFlags::NONBLOCK.bits().cast_signed())
        .open(&lock)?;
    let scratch = Scratch::new()?;
    let with_reader = finish_within(rig.start(&job, &scratch)?, DEADLINE)?;
    assert_eq!(with_reader.code, Some(1), "{}", with_reader.stdout);
    assert!(
        with_reader.stdout.contains("is not a regular file") && with_reader.stdout.contains(&named),
        "{}",
        with_reader.stdout
    );
    Ok(())
}

#[test]
fn deeply_nested_refs_are_replaced_and_the_checkout_succeeds() -> Result<()> {
    let rig = Rig::new()?;
    let next = warm_mirror_then_advance(&rig)?;
    let junk = (0..JUNK_DEPTH).fold(rig.mirror().join("refs/junk"), |path, level| {
        path.join(format!("d{level}"))
    });
    std::fs::create_dir_all(&junk)?;
    std::fs::write(junk.join("stale.lock"), "")?;
    let scratch = Scratch::new()?;
    let ran = rig.run("checkout_depth1", &next, &scratch)?;
    assert_eq!(ran.code, Some(0), "{}\n{}", ran.stdout, ran.stderr);
    assert_eq!(git(&scratch.workspace(), &["rev-parse", "HEAD"])?, next);
    assert!(
        !rig.mirror().join("refs/junk").exists(),
        "the deep refs were kept"
    );
    Ok(())
}
