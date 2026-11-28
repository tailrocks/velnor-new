// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::{Seek, SeekFrom, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::exec_support::{Ran, Scratch};
use crate::native_checkout::{Rig, git};
use crate::native_checkout_env::wrapper_env;

fn pack_of(mirror: &Path) -> Result<PathBuf> {
    for entry in std::fs::read_dir(mirror.join("objects/pack"))? {
        let path = entry?.path();
        if path
            .extension()
            .is_some_and(|extension| extension == "pack")
        {
            return Ok(path);
        }
    }
    anyhow::bail!("no pack in {}", mirror.display())
}

fn corrupt_packed(mirror: &Path, spec: &str) -> Result<String> {
    git(mirror, &["repack", "-adq"])?;
    git(mirror, &["prune-packed"])?;
    let id = git(mirror, &["rev-parse", spec])?;
    let pack = pack_of(mirror)?;
    let listing = git(mirror, &["verify-pack", "-v", &pack.to_string_lossy()])?;
    let offset: u64 = listing
        .lines()
        .find(|line| line.starts_with(&id))
        .and_then(|line| line.split_whitespace().nth(4))
        .context("the object's offset in the pack")?
        .parse()?;
    std::fs::set_permissions(&pack, std::fs::Permissions::from_mode(0o644))?;
    let mut file = std::fs::OpenOptions::new().write(true).open(&pack)?;
    file.seek(SeekFrom::Start(offset + 3))?;
    file.write_all(&[0xff; 4])?;
    Ok(id)
}

fn run_failing(rig: &Rig, id: &str) -> Result<Ran> {
    let ran = rig.run("checkout_depth1", &rig.head, &Scratch::new()?)?;
    let said = format!("{}\n{}", ran.stdout, ran.stderr);
    assert_eq!(ran.code, Some(1), "{said}");
    assert!(
        said.contains(&format!("packed object {id} in pack-"))
            && said.contains("is corrupt; the mirror needs an operator rebuild"),
        "{said}"
    );
    Ok(ran)
}

fn assert_never_refetched_or_leaking(rig: &Rig) -> Result<()> {
    let trace = std::fs::read_to_string(rig.trace())?;
    assert!(!trace.contains("--refetch"), "a refetch ran");
    for entry in std::fs::read_dir(rig.mirror().join("objects/pack"))? {
        let name = entry?.file_name().to_string_lossy().into_owned();
        assert!(!name.starts_with("tmp_"), "{name} leaked");
    }
    Ok(())
}

fn refuses_without_a_refetch(spec_of: fn(&Rig) -> String) -> Result<()> {
    let rig = Rig::new()?;
    let warm = rig.run("checkout_depth1", &rig.head, &Scratch::new()?)?;
    assert_eq!(warm.code, Some(0), "{}\n{}", warm.stdout, warm.stderr);
    let id = corrupt_packed(&rig.mirror(), &spec_of(&rig))?;
    run_failing(&rig, &id)?;
    run_failing(&rig, &id)?;
    assert_never_refetched_or_leaking(&rig)
}

#[test]
fn a_corrupt_packed_blob_fails_the_step_without_a_refetch() -> Result<()> {
    refuses_without_a_refetch(|rig| format!("{}:second", rig.head))
}

#[test]
fn a_corrupt_packed_tree_fails_the_step_without_a_refetch() -> Result<()> {
    refuses_without_a_refetch(|rig| format!("{}^{{tree}}", rig.head))
}

#[test]
fn a_failed_fetch_leaves_no_partial_packs_behind() -> Result<()> {
    let rig = Rig::new()?;
    let packs = rig.mirror().join("objects/pack");
    let keep = packs.join("keep-me");
    wrapper_env(
        &rig.root,
        &format!(
            "case \" $* \" in *\" fetch \"*) : > '{p}/tmp_pack_leaked'; : > '{p}/tmp_idx_leaked'; : > '{k}';; esac\n",
            p = packs.display(),
            k = keep.display()
        ),
    )?;
    let ran = rig.run("checkout_depth1", &"a".repeat(40), &Scratch::new()?)?;
    assert_eq!(ran.code, Some(1), "{}\n{}", ran.stdout, ran.stderr);
    assert!(keep.exists());
    for name in ["tmp_pack_leaked", "tmp_idx_leaked"] {
        assert!(!packs.join(name).exists(), "{name} leaked");
    }
    Ok(())
}
