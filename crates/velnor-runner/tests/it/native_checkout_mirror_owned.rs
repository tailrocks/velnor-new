// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use anyhow::Result;

use crate::native_checkout::Rig;
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

fn in_an_hour(path: &Path) -> Result<()> {
    std::fs::OpenOptions::new()
        .write(true)
        .open(path)?
        .set_modified(SystemTime::now() + Duration::from_hours(1))?;
    Ok(())
}

#[test]
fn a_graft_naming_a_missing_parent_never_wedges_a_job() -> Result<()> {
    let rig = Rig::new()?;
    rig.advance("parent")?;
    let tip = rig.advance("tip")?;
    warm(&rig, &tip)?;
    let grafts = plant(
        &rig.mirror().join("info/grafts"),
        format!("{tip} {}\n", "e".repeat(40)).as_bytes(),
    )?;
    for _ in 0..2 {
        checks_out(&rig, &tip, 3)?;
    }
    assert!(gone(&grafts), "the graft file survived");
    Ok(())
}

#[test]
fn a_packed_refs_directory_never_wedges_a_job() -> Result<()> {
    let rig = Rig::new()?;
    let tip = rig.advance("x")?;
    warm(&rig, &tip)?;
    let packed = rig.mirror().join("packed-refs");
    let _ = std::fs::remove_file(&packed);
    std::fs::create_dir_all(packed.join("sub"))?;
    let next = rig.advance("y")?;
    for depth in [1, 3] {
        checks_out(&rig, &next, depth)?;
    }
    assert!(!packed.is_dir(), "packed-refs is still a directory");
    Ok(())
}

#[test]
fn what_the_broker_does_not_own_is_removed_and_what_it_owns_is_kept() -> Result<()> {
    let rig = Rig::new()?;
    let tip = rig.advance("x")?;
    warm(&rig, &tip)?;
    let mirror = rig.mirror();
    let outside = rig.root.join("outside");
    let target = plant(&outside.join("target"), b"keep me")?;
    std::os::unix::fs::symlink(&outside, mirror.join("linked"))?;
    std::fs::remove_file(mirror.join("FETCH_HEAD"))?;
    std::os::unix::fs::symlink(&target, mirror.join("FETCH_HEAD"))?;
    let strays = [
        plant(&mirror.join("stray"), b"x")?,
        plant(&mirror.join("HEAD.lock"), b"x")?,
        plant(&mirror.join("unknown/nested/file"), b"x")?,
        mirror.join("linked"),
        mirror.join("FETCH_HEAD"),
    ];
    let owned = [
        plant(&mirror.join("quarantine/kept"), b"x")?,
        plant(&mirror.join("sound/kept"), b"x")?,
    ];
    checks_out(&rig, &tip, 2)?;
    for stray in &strays {
        assert!(gone(stray), "{stray:?} survived");
    }
    assert!(gone(&mirror.join("unknown")), "a stray directory survived");
    assert!(target.exists(), "a link was followed");
    for kept in &owned {
        assert!(kept.exists(), "{kept:?} was removed");
    }
    Ok(())
}

#[test]
fn nothing_but_packs_and_loose_objects_stays_in_the_object_store() -> Result<()> {
    let rig = Rig::new()?;
    let tip = rig.advance("x")?;
    warm(&rig, &tip)?;
    let objects = rig.mirror().join("objects");
    let kept = plant(&objects.join("ab/not-a-temporary-file"), b"x")?;
    let strays = [
        plant(&objects.join("info/alternates"), b"/nowhere/objects\n")?,
        plant(&objects.join("stray"), b"x")?,
        plant(&objects.join("not-a-fan-out/file"), b"x")?,
        plant(&objects.join("pack/multi-pack-index"), b"x")?,
        plant(&objects.join("pack/stray"), b"x")?,
    ];
    checks_out(&rig, &tip, 2)?;
    for stray in &strays {
        assert!(gone(stray), "{stray:?} survived");
    }
    assert!(kept.exists(), "a loose-object directory lost its entry");
    Ok(())
}

#[test]
fn a_dead_fetch_leaves_no_temporary_file_in_the_object_store() -> Result<()> {
    let rig = Rig::new()?;
    let tip = rig.advance("x")?;
    warm(&rig, &tip)?;
    let objects = rig.mirror().join("objects");
    let leftovers = [
        plant(&objects.join("c2/tmp_obj_vNByB5"), b"x")?,
        plant(&objects.join("tmp_obj_top"), b"x")?,
        plant(&objects.join("pack/tmp_pack_XXXXXX"), b"x")?,
        plant(&objects.join("pack/tmp_idx_XXXXXX"), b"x")?,
    ];
    checks_out(&rig, &tip, 2)?;
    for leftover in &leftovers {
        assert!(gone(leftover), "{leftover:?} survived");
    }
    Ok(())
}

#[test]
fn a_directory_posing_as_an_orphan_pack_is_removed() -> Result<()> {
    let rig = Rig::new()?;
    let tip = rig.advance("x")?;
    warm(&rig, &tip)?;
    let posing = rig
        .mirror()
        .join(format!("objects/pack/pack-{}.pack", "a".repeat(40)));
    plant(&posing.join("inner/file"), b"x")?;
    checks_out(&rig, &tip, 2)?;
    assert!(gone(&posing), "the directory survived");
    Ok(())
}

#[test]
fn a_temporary_file_newer_than_the_lock_is_left_alone() -> Result<()> {
    let rig = Rig::new()?;
    let tip = rig.advance("x")?;
    warm(&rig, &tip)?;
    let objects = rig.mirror().join("objects");
    let live = [
        plant(&objects.join("c2/tmp_obj_live"), b"x")?,
        plant(&objects.join("tmp_obj_live"), b"x")?,
        plant(&objects.join("pack/tmp_pack_live"), b"x")?,
        plant(&objects.join("pack/tmp_idx_live"), b"x")?,
    ];
    for path in &live {
        in_an_hour(path)?;
    }
    checks_out(&rig, &tip, 2)?;
    for path in &live {
        assert!(path.exists(), "{path:?} was removed");
    }
    Ok(())
}
