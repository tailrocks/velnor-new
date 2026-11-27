// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::exec_support::Scratch;
use crate::native_checkout::{Rig, git};
use crate::native_checkout_env::{real_git, wrapper_env};

pub(crate) fn object_path(repository: &Path, id: &str) -> PathBuf {
    repository.join("objects").join(&id[..2]).join(&id[2..])
}

pub(crate) fn checkout_pull_request(rig: &Rig) -> Result<Scratch> {
    let scratch = Scratch::new()?;
    let ran = rig.run("checkout_pull_request", &rig.merge, &scratch)?;
    assert_eq!(ran.code, Some(0), "{}\n{}", ran.stdout, ran.stderr);
    Ok(scratch)
}

pub(crate) fn assert_workspace_intact(scratch: &Scratch) -> Result<()> {
    let workspace = scratch.workspace();
    git(&workspace, &["status", "--porcelain"])?;
    git(&workspace, &["cat-file", "-e", "HEAD^{tree}"])?;
    Ok(())
}

fn repairs_after_losing(rig: &Rig, spec: &str, fetches: usize) -> Result<()> {
    let live = checkout_pull_request(rig)?;
    let mirror = rig.mirror();
    let id = git(&mirror, &["rev-parse", spec])?;
    let object = object_path(&mirror, &id);
    std::fs::remove_file(&object).with_context(|| format!("remove {}", object.display()))?;
    let scratch = Scratch::new()?;
    let ran = rig.run("checkout_depth1", &rig.head, &scratch)?;
    assert_eq!(ran.code, Some(0), "{}\n{}", ran.stdout, ran.stderr);
    assert_eq!(git(&scratch.workspace(), &["rev-parse", "HEAD"])?, rig.head);
    assert_eq!(
        std::fs::read_to_string(scratch.workspace().join("second"))?,
        "second"
    );
    assert_eq!(rig.fetches(), fetches);
    git(&mirror, &["cat-file", "-e", &id])?;
    assert_workspace_intact(&live)
}

#[test]
fn a_mirror_missing_a_blob_is_repaired_in_place() -> Result<()> {
    let rig = Rig::new()?;
    let spec = format!("{}:second", rig.head);
    repairs_after_losing(&rig, &spec, 4)
}

#[test]
fn a_mirror_missing_a_tree_is_repaired_in_place() -> Result<()> {
    let rig = Rig::new()?;
    let spec = format!("{}^{{tree}}", rig.head);
    repairs_after_losing(&rig, &spec, 4)
}

#[test]
fn a_mirror_missing_the_commit_is_repaired_in_place() -> Result<()> {
    let rig = Rig::new()?;
    let head = rig.head.clone();
    repairs_after_losing(&rig, &head, 4)
}

#[test]
fn repairing_the_mirror_leaves_other_live_workspaces_intact() -> Result<()> {
    let rig = Rig::new()?;
    let live = checkout_pull_request(&rig)?;
    let next = rig.advance("third")?;
    let planted = object_path(&rig.mirror(), &next);
    let source = object_path(&rig.root.join("work/.git"), &next);
    std::fs::create_dir_all(planted.parent().context("object directory")?)?;
    std::fs::copy(&source, &planted)?;
    let scratch = Scratch::new()?;
    let ran = rig.run("checkout_depth1", &next, &scratch)?;
    assert_eq!(ran.code, Some(0), "{}\n{}", ran.stdout, ran.stderr);
    assert_eq!(git(&scratch.workspace(), &["rev-parse", "HEAD"])?, next);
    assert_workspace_intact(&live)?;
    assert_eq!(git(&live.workspace(), &["rev-parse", "HEAD"])?, rig.merge);
    Ok(())
}

#[test]
fn a_server_that_sends_the_commit_without_its_tree_fails_the_step() -> Result<()> {
    let rig = Rig::new()?;
    let tree = git(
        &rig.root.join("work"),
        &["rev-parse", &format!("{}^{{tree}}", rig.head)],
    )?;
    let object = object_path(&rig.mirror(), &tree);
    wrapper_env(
        &rig.root,
        &format!(
            "case \" $* \" in *\" fetch \"*) '{real}' \"$@\"; s=$?; rm -f '{object}'; exit $s;; esac\n",
            real = real_git()?,
            object = object.display()
        ),
    )?;
    let scratch = Scratch::new()?;
    let ran = rig.run("checkout_depth1", &rig.head, &scratch)?;
    assert_eq!(ran.code, Some(1), "{}\n{}", ran.stdout, ran.stderr);
    assert!(
        ran.stdout.contains("arrived without all of its objects")
            || ran.stderr.contains("arrived without all of its objects"),
        "{}\n{}",
        ran.stdout,
        ran.stderr
    );
    assert!(!scratch.workspace().join(".git").exists());
    Ok(())
}

pub(crate) fn overwrite(object: &Path, bytes: &[u8]) -> Result<()> {
    std::fs::set_permissions(object, std::fs::Permissions::from_mode(0o644))?;
    std::fs::write(object, bytes).with_context(|| format!("overwrite {}", object.display()))
}

#[test]
fn a_corrupt_loose_blob_is_quarantined_refetched_and_the_checkout_retried() -> Result<()> {
    let bytes = b"not a zlib stream";
    let rig = Rig::new()?;
    let live = checkout_pull_request(&rig)?;
    let mirror = rig.mirror();
    let id = git(&mirror, &["rev-parse", &format!("{}:second", rig.head)])?;
    let object = object_path(&mirror, &id);
    overwrite(&object, bytes)?;
    let scratch = Scratch::new()?;
    let ran = rig.run("checkout_depth1", &rig.head, &scratch)?;
    assert_eq!(ran.code, Some(0), "{}\n{}", ran.stdout, ran.stderr);
    assert_eq!(git(&scratch.workspace(), &["rev-parse", "HEAD"])?, rig.head);
    assert_eq!(
        std::fs::read_to_string(scratch.workspace().join("second"))?,
        "second"
    );
    let quarantined: Vec<String> = std::fs::read_dir(mirror.join("quarantine"))?
        .map(|entry| Ok(entry?.file_name().to_string_lossy().into_owned()))
        .collect::<Result<_>>()?;
    assert_eq!(quarantined.len(), 1, "{quarantined:?}");
    assert!(quarantined[0].starts_with(&id), "{quarantined:?}");
    assert_eq!(
        std::fs::read(mirror.join("quarantine").join(&quarantined[0]))?,
        bytes
    );
    git(&mirror, &["cat-file", "blob", &id])?;
    assert_eq!(rig.fetches(), 2);
    assert_workspace_intact(&live)?;
    assert_eq!(git(&live.workspace(), &["show", "HEAD:second"])?, "second");
    Ok(())
}

#[test]
fn a_checkout_that_fails_on_a_sound_mirror_neither_refetches_nor_quarantines() -> Result<()> {
    let rig = Rig::new()?;
    wrapper_env(
        &rig.root,
        "case \" $* \" in *\" checkout-index \"*) echo checkout-refused >&2; exit 1;; esac\n",
    )?;
    let scratch = Scratch::new()?;
    let ran = rig.run("checkout_depth1", &rig.head, &scratch)?;
    assert_eq!(ran.code, Some(1), "{}\n{}", ran.stdout, ran.stderr);
    assert!(
        ran.stdout.contains("checkout: git checkout failed")
            || ran.stderr.contains("checkout: git checkout failed"),
        "{}\n{}",
        ran.stdout,
        ran.stderr
    );
    assert_eq!(rig.fetches(), 1);
    assert!(!rig.mirror().join("quarantine").exists());
    Ok(())
}
