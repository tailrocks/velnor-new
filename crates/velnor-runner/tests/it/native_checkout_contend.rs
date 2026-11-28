// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::exec_support::Scratch;
use crate::native_checkout::{Rig, finish, git};
use crate::native_checkout_quarantine::quarantined;
use crate::native_checkout_repair::{object_path, overwrite};
use crate::native_checkout_walk::{checks_out, warm};

const GARBAGE: &[u8] = b"not a zlib stream";

fn corrupt_blob_of(rig: &Rig, tip: &str) -> Result<()> {
    let blob = git(&rig.mirror(), &["rev-parse", &format!("{tip}:x")])?;
    overwrite(&object_path(&rig.mirror(), &blob), GARBAGE)
}

fn mark_sound(rig: &Rig, name: &str) -> Result<PathBuf> {
    let directory = rig.mirror().join("sound");
    std::fs::create_dir_all(&directory)?;
    let marker = directory.join(name);
    std::fs::write(&marker, b"")?;
    Ok(marker)
}

#[test]
fn jobs_queued_behind_a_repair_check_out_after_it() -> Result<()> {
    let rig = Rig::new()?;
    let tip = rig.advance("x")?;
    warm(&rig, &tip)?;
    corrupt_blob_of(&rig, &tip)?;
    let job = rig.job("checkout_depth1", &tip, &rig.server_url())?;
    let scratches: Vec<Scratch> = (0..4).map(|_| Scratch::new()).collect::<Result<_>>()?;
    let children: Vec<_> = scratches
        .iter()
        .map(|scratch| rig.start(&job, scratch))
        .collect::<Result<_>>()?;
    let mut codes = Vec::new();
    for child in children {
        codes.push(finish(child)?.code);
    }
    assert_eq!(codes, vec![Some(0); 4]);
    for scratch in &scratches {
        assert_eq!(git(&scratch.workspace(), &["rev-parse", "HEAD"])?, tip);
        assert_eq!(std::fs::read_to_string(scratch.workspace().join("x"))?, "x");
    }
    assert_eq!(rig.refetches(), 1);
    Ok(())
}

#[test]
fn a_sound_mark_does_not_hide_damage_to_the_commits_own_objects() -> Result<()> {
    let rig = Rig::new()?;
    let tip = rig.advance("x")?;
    warm(&rig, &tip)?;
    mark_sound(&rig, &format!("{tip}-last-1"))?;
    corrupt_blob_of(&rig, &tip)?;
    for _ in 0..2 {
        checks_out(&rig, &tip, 1)?;
    }
    assert_eq!(rig.refetches(), 1);
    assert_eq!(quarantined(&rig).len(), 1);
    Ok(())
}

#[test]
fn a_refetch_forgets_every_sound_mark() -> Result<()> {
    let rig = Rig::new()?;
    let tip = rig.advance("x")?;
    warm(&rig, &tip)?;
    let other = mark_sound(&rig, &format!("{}-all", rig.head))?;
    corrupt_blob_of(&rig, &tip)?;
    checks_out(&rig, &tip, 1)?;
    assert_eq!(rig.refetches(), 1);
    assert!(!other.exists(), "the mark of another commit survived");
    Ok(())
}

#[test]
fn an_orphan_pack_is_removed_and_a_whole_pack_is_kept() -> Result<()> {
    let rig = Rig::new()?;
    let tip = rig.advance("x")?;
    warm(&rig, &tip)?;
    git(&rig.mirror(), &["repack", "-a", "-d", "-q"])?;
    let packs = rig.mirror().join("objects/pack");
    let whole: Vec<String> = std::fs::read_dir(&packs)?
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with("pack-"))
        .collect();
    assert!(
        whole
            .iter()
            .any(|name| Path::new(name).extension().is_some_and(|ext| ext == "idx")),
        "{whole:?}"
    );
    let orphan = packs.join(format!("pack-{}.pack", "0".repeat(40)));
    std::fs::write(&orphan, b"PACKjunk")?;
    checks_out(&rig, &tip, 2)?;
    assert!(!orphan.exists(), "the orphan pack survived");
    for name in &whole {
        assert!(packs.join(name).exists(), "{name} was removed");
    }
    Ok(())
}
