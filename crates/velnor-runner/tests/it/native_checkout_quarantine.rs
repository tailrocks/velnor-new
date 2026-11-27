// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use anyhow::Result;

use crate::exec_support::Scratch;
use crate::native_checkout::{Rig, git};
use crate::native_checkout_env::{real_git, wrapper_env};
use crate::native_checkout_repair::{assert_workspace_intact, object_path, overwrite};

pub(crate) fn quarantined(rig: &Rig) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(rig.mirror().join("quarantine")) else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect()
}

#[test]
fn a_missing_ancestor_and_a_corrupt_blob_quarantine_only_the_blob() -> Result<()> {
    let rig = Rig::new()?;
    let live = Scratch::new()?;
    let warm = rig.run("checkout_depth1", &rig.head, &live)?;
    assert_eq!(warm.code, Some(0), "{}\n{}", warm.stdout, warm.stderr);
    let mirror = rig.mirror();
    let parent = git(&mirror, &["rev-parse", &format!("{}~1", rig.head)])?;
    std::fs::remove_file(object_path(&mirror, &parent))?;
    let blob = git(&mirror, &["rev-parse", &format!("{}:second", rig.head)])?;
    overwrite(&object_path(&mirror, &blob), b"not a zlib stream")?;
    let scratch = Scratch::new()?;
    let ran = rig.run("checkout_depth1", &rig.head, &scratch)?;
    assert_eq!(ran.code, Some(0), "{}\n{}", ran.stdout, ran.stderr);
    assert_eq!(git(&scratch.workspace(), &["rev-parse", "HEAD"])?, rig.head);
    let moved = quarantined(&rig);
    assert_eq!(moved.len(), 1, "{moved:?}");
    assert!(moved[0].starts_with(&blob), "{moved:?}");
    git(&mirror, &["cat-file", "-e", &rig.head])?;
    assert_workspace_intact(&live)?;
    assert_eq!(git(&live.workspace(), &["rev-parse", "HEAD"])?, rig.head);
    Ok(())
}

#[test]
fn a_sound_object_named_by_a_damage_report_is_not_quarantined() -> Result<()> {
    let rig = Rig::new()?;
    let warm = rig.run("checkout_depth1", &rig.head, &Scratch::new()?)?;
    assert_eq!(warm.code, Some(0), "{}\n{}", warm.stdout, warm.stderr);
    let blob = git(
        &rig.mirror(),
        &["rev-parse", &format!("{}:second", rig.head)],
    )?;
    let once = rig.root.join("refused-once");
    wrapper_env(
        &rig.root,
        &format!(
            "case \" $* \" in\n\
             *\" checkout-index \"*) [ -f '{once}' ] || {{ : > '{once}'; echo checkout-refused >&2; exit 1; }};;\n\
             *\" --verify-objects \"*\" --stdin \"*) listed=$(cat); case \"$listed\" in *{head}*) echo 'error: hash mismatch {blob}' >&2; exit 1;; esac; printf '%s\\n' \"$listed\" | '{real}' \"$@\"; exit $?;;\n\
             esac\n",
            once = once.display(),
            head = rig.head,
            real = real_git()?
        ),
    )?;
    let scratch = Scratch::new()?;
    let ran = rig.run("checkout_depth1", &rig.head, &scratch)?;
    assert_eq!(ran.code, Some(0), "{}\n{}", ran.stdout, ran.stderr);
    assert_eq!(git(&scratch.workspace(), &["rev-parse", "HEAD"])?, rig.head);
    assert_eq!(quarantined(&rig), Vec::<String>::new());
    git(&rig.mirror(), &["cat-file", "blob", &blob])?;
    Ok(())
}

fn warm_then_damage(
    rig: &Rig,
    spec: &str,
    damage: impl FnOnce(&std::path::Path, &str) -> Result<()>,
) -> Result<String> {
    let live = Scratch::new()?;
    let warm = rig.run("checkout_depth1", &rig.head, &live)?;
    assert_eq!(warm.code, Some(0), "{}\n{}", warm.stdout, warm.stderr);
    let mirror = rig.mirror();
    let id = git(&mirror, &["rev-parse", spec])?;
    damage(&mirror, &id)?;
    Ok(id)
}

fn assert_repaired(rig: &Rig, id: &str) -> Result<()> {
    let scratch = Scratch::new()?;
    let ran = rig.run("checkout_depth1", &rig.head, &scratch)?;
    assert_eq!(ran.code, Some(0), "{}\n{}", ran.stdout, ran.stderr);
    let workspace = scratch.workspace();
    assert_eq!(git(&workspace, &["rev-parse", "HEAD"])?, rig.head);
    assert_eq!(std::fs::read_to_string(workspace.join("second"))?, "second");
    assert_eq!(git(&workspace, &["status", "--porcelain"])?, "");
    let moved = quarantined(rig);
    assert_eq!(moved.len(), 1, "{moved:?}");
    assert!(moved[0].starts_with(id), "{moved:?}");
    git(&rig.mirror(), &["cat-file", "-e", id])?;
    Ok(())
}

fn empty_object(mirror: &std::path::Path, id: &str) -> Result<()> {
    overwrite(&object_path(mirror, id), b"")
}

#[test]
fn an_empty_loose_blob_is_repaired_and_the_workspace_is_complete() -> Result<()> {
    let rig = Rig::new()?;
    let blob = warm_then_damage(&rig, &format!("{}:second", rig.head), empty_object)?;
    assert_repaired(&rig, &blob)
}

#[test]
fn an_empty_root_tree_of_the_job_commit_is_repaired_by_the_fetch() -> Result<()> {
    let rig = Rig::new()?;
    let tree = warm_then_damage(&rig, &format!("{}^{{tree}}", rig.head), empty_object)?;
    assert_repaired(&rig, &tree)
}

#[test]
fn an_empty_job_commit_is_repaired_by_the_fetch() -> Result<()> {
    let rig = Rig::new()?;
    let commit = warm_then_damage(&rig, &rig.head, empty_object)?;
    assert_repaired(&rig, &commit)
}

#[test]
fn a_loose_blob_replaced_by_an_object_of_another_type_is_repaired() -> Result<()> {
    let rig = Rig::new()?;
    let blob = warm_then_damage(&rig, &format!("{}:second", rig.head), |mirror, blob| {
        let tree = git(mirror, &["rev-parse", &format!("{}^{{tree}}", rig.head)])?;
        std::fs::remove_file(object_path(mirror, blob))?;
        std::fs::copy(object_path(mirror, &tree), object_path(mirror, blob))?;
        Ok(())
    })?;
    assert_repaired(&rig, &blob)
}
