// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::Result;

use crate::native_checkout::{Rig, git};
use crate::native_checkout_walk::{checks_out, warm};

fn plant(path: &Path, body: &[u8]) -> Result<PathBuf> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, body)?;
    Ok(path.to_owned())
}

fn gone(path: &Path) -> bool {
    path.symlink_metadata().is_err()
}

fn warmed() -> Result<Rig> {
    let rig = Rig::new()?;
    let tip = rig.advance("warm")?;
    warm(&rig, &tip)?;
    Ok(rig)
}

fn follows_upstream(rig: &Rig, name: &str) -> Result<()> {
    let next = rig.advance(name)?;
    checks_out(rig, &next, 1)?;
    assert_eq!(git(&rig.mirror(), &["rev-parse", "refs/heads/main"])?, next);
    Ok(())
}

#[test]
fn a_ref_that_holds_no_ref_is_removed_and_a_real_one_is_kept() -> Result<()> {
    let rig = warmed()?;
    let extra = rig.mirror().join("refs/extra");
    std::fs::create_dir_all(&extra)?;
    let tip = git(&rig.mirror(), &["rev-parse", "refs/heads/main"])?;
    let outside = plant(&rig.root.join("outside/target"), b"keep me")?;
    let fifo = extra.join("fifo");
    assert!(Command::new("mkfifo").arg(&fifo).status()?.success());
    std::os::unix::fs::symlink(&outside, extra.join("link"))?;
    let strays = [
        plant(&extra.join("empty"), b"")?,
        plant(&extra.join("garbage"), b"not a ref\n")?,
        plant(&extra.join("long"), "a".repeat(400).as_bytes())?,
        plant(&extra.join("deep/empty"), b"")?,
        extra.join("link"),
        fifo,
    ];
    let kept = [
        plant(&extra.join("copy"), format!("{tip}\n").as_bytes())?,
        plant(&extra.join("deep/copy"), tip.as_bytes())?,
        plant(&extra.join("symbolic"), b"ref: refs/heads/main\n")?,
    ];
    follows_upstream(&rig, "next")?;
    for stray in &strays {
        assert!(gone(stray), "{stray:?} survived");
    }
    for path in &kept {
        assert!(path.exists(), "{path:?} was removed");
    }
    assert_eq!(std::fs::read(&outside)?, b"keep me");
    Ok(())
}

#[test]
fn a_directory_where_a_ref_belongs_does_not_freeze_the_mirror() -> Result<()> {
    let rig = warmed()?;
    let main = rig.mirror().join("refs/heads/main");
    std::fs::remove_file(&main)?;
    let _ = std::fs::remove_file(rig.mirror().join("packed-refs"));
    plant(&main.join("x"), b"")?;
    follows_upstream(&rig, "next")?;
    assert!(main.is_file(), "{main:?} is not a ref");
    Ok(())
}

#[test]
fn a_link_where_the_branch_directory_belongs_is_replaced_and_never_followed() -> Result<()> {
    let rig = warmed()?;
    let heads = rig.mirror().join("refs/heads");
    let outside = rig.root.join("outside-heads");
    std::fs::create_dir_all(&outside)?;
    std::fs::remove_dir_all(&heads)?;
    std::os::unix::fs::symlink(&outside, &heads)?;
    follows_upstream(&rig, "next")?;
    assert!(heads.is_dir() && !heads.is_symlink(), "the link survived");
    assert_eq!(
        std::fs::read_dir(&outside)?.count(),
        0,
        "a link was followed"
    );
    Ok(())
}

#[test]
fn a_lock_of_any_type_is_removed_without_following_it() -> Result<()> {
    let rig = warmed()?;
    let heads = rig.mirror().join("refs/heads");
    let outside = plant(&rig.root.join("outside/target"), b"keep me")?;
    let tip = git(&rig.mirror(), &["rev-parse", "refs/heads/main"])?;
    plant(&heads.join("main.lock/inner/file"), tip.as_bytes())?;
    plant(&heads.join("other.lock"), tip.as_bytes())?;
    plant(&heads.join("deep/third.lock"), tip.as_bytes())?;
    std::os::unix::fs::symlink(&outside, heads.join("linked.lock"))?;
    std::os::unix::fs::symlink(rig.root.join("nowhere"), heads.join("dangling.lock"))?;
    follows_upstream(&rig, "next")?;
    for name in [
        "main.lock",
        "other.lock",
        "deep/third.lock",
        "linked.lock",
        "dangling.lock",
    ] {
        assert!(gone(&heads.join(name)), "{name} survived");
    }
    assert_eq!(std::fs::read(&outside)?, b"keep me");
    Ok(())
}
