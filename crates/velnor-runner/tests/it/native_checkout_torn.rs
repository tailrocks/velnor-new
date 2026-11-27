// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use anyhow::Result;

use crate::native_checkout::{Rig, commit, git};
use crate::native_checkout_quarantine::quarantined;
use crate::native_checkout_repair::{object_path, overwrite};
use crate::native_checkout_walk::{checks_out, publish, warm};

fn truncated(rig: &Rig, id: &str) -> Result<()> {
    let path = object_path(&rig.mirror(), id);
    assert!(path.exists(), "{id} is not a loose object");
    let bytes = std::fs::read(&path)?;
    overwrite(&path, &bytes[..bytes.len() - 6])
}

fn moved(rig: &Rig, ids: &[&str]) -> bool {
    let names = quarantined(rig);
    names.len() == ids.len()
        && ids
            .iter()
            .all(|id| names.iter().any(|name| name.starts_with(id)))
}

fn merge_of_two_parents(rig: &Rig) -> Result<(String, String, String)> {
    let work = rig.root.join("work");
    git(&work, &["checkout", "-q", "-B", "p1", &rig.head])?;
    let p1 = commit(&work, "p1")?;
    git(&work, &["checkout", "-q", "main"])?;
    let p2 = commit(&work, "p2")?;
    let tree = git(&work, &["rev-parse", &format!("{p2}^{{tree}}")])?;
    let merge = git(
        &work,
        &["commit-tree", &tree, "-p", &p1, "-p", &p2, "-m", "m"],
    )?;
    git(&work, &["reset", "-q", "--hard", &merge])?;
    git(&work, &["branch", "-D", "p1"])?;
    publish(rig)?;
    Ok((merge, p1, p2))
}

#[test]
fn a_truncated_parent_commit_is_repaired_at_depth_three() -> Result<()> {
    let rig = Rig::new()?;
    let parent = rig.advance("parent")?;
    let tip = rig.advance("tip")?;
    warm(&rig, &tip)?;
    truncated(&rig, &parent)?;
    for _ in 0..2 {
        checks_out(&rig, &tip, 3)?;
    }
    assert_eq!(rig.refetches(), 1);
    assert!(moved(&rig, &[&parent]), "{:?}", quarantined(&rig));
    git(&rig.mirror(), &["cat-file", "-p", &parent])?;
    Ok(())
}

#[test]
fn every_truncated_commit_of_one_level_is_found_in_one_scan() -> Result<()> {
    let rig = Rig::new()?;
    let (merge, p1, p2) = merge_of_two_parents(&rig)?;
    warm(&rig, &merge)?;
    truncated(&rig, &p1)?;
    truncated(&rig, &p2)?;
    for _ in 0..2 {
        checks_out(&rig, &merge, 3)?;
    }
    assert_eq!(rig.refetches(), 1);
    assert!(moved(&rig, &[&p1, &p2]), "{:?}", quarantined(&rig));
    Ok(())
}
