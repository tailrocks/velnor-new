// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use anyhow::Result;

use crate::native_checkout::Rig;
use crate::native_checkout_walk::{checks_out, warm};

fn warmed(rig: &Rig) -> Result<String> {
    let tip = rig.advance("x")?;
    warm(rig, &tip)?;
    Ok(tip)
}

fn is_real_directory(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|meta| meta.is_dir())
}

#[test]
fn a_mirror_whose_head_is_a_directory_is_repaired_for_every_job() -> Result<()> {
    let rig = Rig::new()?;
    let tip = warmed(&rig)?;
    let head = rig.mirror().join("HEAD");
    std::fs::remove_file(&head)?;
    std::fs::create_dir_all(head.join("sub"))?;
    for _ in 0..2 {
        checks_out(&rig, &tip, 2)?;
    }
    assert_eq!(std::fs::read_to_string(&head)?, "ref: refs/heads/main\n");
    Ok(())
}

#[test]
fn a_symlinked_pack_directory_is_replaced_and_what_it_pointed_at_is_untouched() -> Result<()> {
    let rig = Rig::new()?;
    let tip = warmed(&rig)?;
    let outside = rig.root.join("outside");
    std::fs::create_dir_all(&outside)?;
    let victim = outside.join(format!("pack-{}.pack", "1".repeat(40)));
    std::fs::write(&victim, b"not a pack, only named like one")?;
    let packs = rig.mirror().join("objects/pack");
    std::fs::remove_dir_all(&packs)?;
    std::os::unix::fs::symlink(&outside, &packs)?;
    checks_out(&rig, &tip, 2)?;
    assert!(victim.exists(), "the sweep followed the symlink");
    assert!(
        is_real_directory(&packs),
        "the mirror kept writing through the symlink"
    );
    Ok(())
}

#[test]
fn an_orphan_pack_takes_every_sidecar_with_it() -> Result<()> {
    let rig = Rig::new()?;
    let tip = warmed(&rig)?;
    let packs = rig.mirror().join("objects/pack");
    let stem = format!("pack-{}", "0".repeat(40));
    let leftovers: Vec<_> = ["pack", "rev", "mtimes", "keep"]
        .iter()
        .map(|extension| packs.join(format!("{stem}.{extension}")))
        .collect();
    for leftover in &leftovers {
        std::fs::write(leftover, b"left by a crash")?;
    }
    checks_out(&rig, &tip, 2)?;
    for leftover in &leftovers {
        assert!(
            leftover.symlink_metadata().is_err(),
            "{leftover:?} survived"
        );
    }
    Ok(())
}
