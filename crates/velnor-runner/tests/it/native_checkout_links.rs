// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::os::unix::fs::symlink;
use std::path::Path;

use anyhow::{Context, Result};

use crate::exec_support::Scratch;
use crate::native_checkout::{Rig, git};

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
    std::fs::write(&target, "ref: refs/heads/main\n")?;
    replace_with_link(&rig.mirror().join("HEAD"), &target)?;
    let scratch = Scratch::new()?;
    let ran = rig.run("checkout_depth1", &next, &scratch)?;
    assert_eq!(ran.code, Some(0), "{}\n{}", ran.stdout, ran.stderr);
    assert_eq!(std::fs::read_to_string(&target)?, "ref: refs/heads/main\n");
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
