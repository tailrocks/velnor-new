// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use anyhow::{Context, Result};

use crate::exec_support::Scratch;
use crate::native_checkout::{Rig, git};

fn rebuilds_after_losing(rig: &Rig, spec: &str) -> Result<()> {
    let warm = Scratch::new()?;
    let ran = rig.run("checkout_depth1", &rig.head, &warm)?;
    assert_eq!(ran.code, Some(0), "{}\n{}", ran.stdout, ran.stderr);
    let mirror = rig.mirror();
    let id = git(&mirror, &["rev-parse", spec])?;
    let object = mirror.join("objects").join(&id[..2]).join(&id[2..]);
    std::fs::remove_file(&object).with_context(|| format!("remove {}", object.display()))?;
    let scratch = Scratch::new()?;
    let ran = rig.run("checkout_depth1", &rig.head, &scratch)?;
    assert_eq!(ran.code, Some(0), "{}\n{}", ran.stdout, ran.stderr);
    assert_eq!(git(&scratch.workspace(), &["rev-parse", "HEAD"])?, rig.head);
    assert_eq!(
        std::fs::read_to_string(scratch.workspace().join("second"))?,
        "second"
    );
    assert_eq!(rig.fetches(), 2);
    assert!(object.exists(), "the lost object was not restored");
    Ok(())
}

#[test]
fn a_mirror_missing_a_blob_is_rebuilt_once() -> Result<()> {
    let rig = Rig::new()?;
    let spec = format!("{}:second", rig.head);
    rebuilds_after_losing(&rig, &spec)
}

#[test]
fn a_mirror_missing_a_tree_is_rebuilt_once() -> Result<()> {
    let rig = Rig::new()?;
    let spec = format!("{}^{{tree}}", rig.head);
    rebuilds_after_losing(&rig, &spec)
}
