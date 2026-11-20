// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use super::input_scopes::{git_prefix, scoped_files};
pub(super) use super::inputs_git::{git, modified_ns, set_modified_ns};
mod sources;
use crate::error::CiError;
use crate::inputs::{
    SavedInput, Tracked, format_inputs, inputs_file, plan_restore, restored_units_file,
    snapshot_file, witness_file,
};
use crate::scope::Scope;
pub(super) use sources::{
    TrackedPaths, capture_inputs, source_files, top_level_files, tracked_paths,
};

static NEXT_SNAPSHOT: AtomicU64 = AtomicU64::new(0);

pub fn save_inputs(root: &Path, target: &Path) -> Result<String, CiError> {
    save_inputs_scoped(root, target, None)
}

pub(super) fn save_inputs_scoped(
    root: &Path,
    target: &Path,
    scope: Option<&Scope>,
) -> Result<String, CiError> {
    let (top, tracked) = scoped_files(root, scope)?;
    let identity =
        super::delta_source::identity_of(&top_level_files(&top, &Scope::default(), false)?)
            .unwrap_or_default();
    let relative = target.join(inputs_file(&git_prefix(root)?));
    let path = root.join(&relative);
    check_snapshot_parent(&path)?;
    if std::env::var("EXPECTED_SOURCE_IDENTITY")
        .is_ok_and(|expected| identity.is_empty() || expected != identity)
    {
        super::delta_snapshot::replace(&path, None)?;
        return Ok("saved nothing: source identity changed or unavailable\n".to_owned());
    }
    if tracked.unverifiable_links > 0 {
        match fs::remove_file(&path) {
            Ok(()) => {}
            Err(e) if e.kind() == ErrorKind::NotFound => {}
            Err(e) => {
                return Err(CiError::new(format!(
                    "cannot remove {}: {e}",
                    path.display()
                )));
            }
        }
        return Ok(format!(
            "saved nothing: {} tracked symlinks have a path or target the file cannot hold, so restore finds no saved inputs\n",
            tracked.unverifiable_links
        ));
    }
    let mut saved = Vec::with_capacity(tracked.files.len() + tracked.links.len());
    for file in &tracked.files {
        saved.push(SavedInput {
            file: file.clone(),
            mtime_ns: modified_ns(&top.join(&file.path))?,
        });
    }
    saved.extend(tracked.links.iter().map(|link| SavedInput {
        file: link.clone(),
        mtime_ns: 0,
    }));
    publish_snapshot(&path, &super::input_snapshot::encode(&identity, &saved))?;
    Ok(format!(
        "saved {} tracked files and {} symlinks to {}; left out: {} not in the worktree, {} with a path the file cannot hold, {} submodules not checked out\n",
        tracked.files.len(),
        tracked.links.len(),
        relative.display(),
        tracked.absent.len(),
        tracked.unsupported,
        tracked.submodules
    ))
}

pub(super) fn invalidate_snapshot(root: &Path, target: &Path) -> Result<(), CiError> {
    let path = root.join(target).join(inputs_file(&git_prefix(root)?));
    check_snapshot_parent(&path)?;
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(CiError::new(format!(
            "cannot invalidate {}: {error}",
            path.display()
        ))),
    }
}

fn check_snapshot_parent(path: &Path) -> Result<(), CiError> {
    let parent = path
        .parent()
        .ok_or_else(|| CiError::new(format!("{} has no parent", path.display())))?;
    match fs::symlink_metadata(parent) {
        Ok(metadata) if metadata.is_dir() => Ok(()),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
        Ok(_) => Err(CiError::new(format!(
            "snapshot directory {} is not a regular directory",
            parent.display()
        ))),
        Err(e) => Err(CiError::new(format!(
            "cannot inspect snapshot directory {}: {e}",
            parent.display()
        ))),
    }
}

pub(super) fn publish_snapshot(path: &Path, text: &str) -> Result<(), CiError> {
    let parent = path
        .parent()
        .ok_or_else(|| CiError::new(format!("{} has no parent", path.display())))?;
    fs::create_dir_all(parent)
        .map_err(|e| CiError::new(format!("cannot create {}: {e}", parent.display())))?;
    check_snapshot_parent(path)?;
    let (staged, mut file) = loop {
        let sequence = NEXT_SNAPSHOT.fetch_add(1, Ordering::Relaxed);
        let staged = parent.join(format!(".inputs.{}.{sequence}.tmp", std::process::id()));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&staged)
        {
            Ok(file) => break (staged, file),
            Err(e) if e.kind() == ErrorKind::AlreadyExists => {}
            Err(e) => {
                return Err(CiError::new(format!(
                    "cannot create {}: {e}",
                    staged.display()
                )));
            }
        }
    };
    let written = file.write_all(text.as_bytes());
    drop(file);
    let published = written.and_then(|()| fs::rename(&staged, path));
    if let Err(error) = published {
        fs::remove_file(&staged)
            .map_err(|e| CiError::new(format!("cannot remove {}: {e}", staged.display())))?;
        return Err(CiError::new(format!(
            "cannot write {}: {error}",
            path.display()
        )));
    }
    Ok(())
}

pub(super) fn input_witness(tracked: &Tracked) -> String {
    let mut inputs: Vec<_> = tracked
        .files
        .iter()
        .chain(&tracked.links)
        .cloned()
        .map(|file| SavedInput { file, mtime_ns: 0 })
        .collect();
    inputs.extend(tracked.absent.iter().map(|path| SavedInput {
        file: crate::inputs::TrackedFile {
            path: path.clone(),
            blob: String::new(),
            mode: "absent".to_owned(),
        },
        mtime_ns: 0,
    }));
    for (marker, count) in [
        ("unsupported", tracked.unsupported),
        ("unverifiable-link", tracked.unverifiable_links),
        ("submodule", tracked.submodules),
    ] {
        if count > 0 {
            inputs.push(SavedInput {
                file: crate::inputs::TrackedFile {
                    path: format!("@{marker}"),
                    blob: count.to_string(),
                    mode: "marker".to_owned(),
                },
                mtime_ns: 0,
            });
        }
    }
    inputs.sort_by(|left, right| left.file.cmp(&right.file));
    format_inputs(&inputs)
}

pub fn restore_inputs(root: &Path, target: &Path) -> Result<String, CiError> {
    restore_inputs_scoped(root, target, None)
}

pub(super) fn restore_inputs_scoped(
    root: &Path,
    target: &Path,
    scope: Option<&Scope>,
) -> Result<String, CiError> {
    let prefix = git_prefix(root)?;
    remove_restored(&root.join(target).join(snapshot_file(&prefix)))?;
    remove_restored(&root.join(target).join(witness_file(&prefix)))?;
    remove_restored(&root.join(target).join(restored_units_file(&prefix)))?;
    let relative = target.join(inputs_file(&prefix));
    let path = root.join(&relative);
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == ErrorKind::NotFound => {
            return Err(CiError::new(format!(
                "no {}; cannot restore cached inputs",
                relative.display()
            )));
        }
        Err(e) => return Err(CiError::new(format!("cannot read {}: {e}", path.display()))),
    };
    let saved = super::input_snapshot::decode(&text)
        .map_err(|e| CiError::new(format!("{}: {e}", path.display())))?
        .inputs;
    let (top, tracked) = scoped_files(root, scope)?;
    fs::write(
        root.join(target).join(witness_file(&prefix)),
        input_witness(&tracked),
    )
    .map_err(|e| CiError::new(format!("cannot preserve input witness: {e}")))?;
    let plan = plan_restore(&saved, &tracked);
    if plan.links_changed > 0 {
        return Err(CiError::new(format!(
            "restored nothing: {} tracked symlinks are new, changed or deleted since the save\n",
            plan.links_changed
        )));
    }
    fs::write(root.join(target).join(snapshot_file(&prefix)), &text)
        .map_err(|e| CiError::new(format!("cannot preserve restored inputs: {e}")))?;
    super::restored_units::record(&root.join(target), &prefix)?;
    for input in &plan.restore {
        set_modified_ns(&top.join(&input.file.path), input.mtime_ns)?;
    }
    Ok(format!(
        "restored the modification time of {} unchanged files; {} changed, {} new, {} deleted, {} not in the worktree, {} with a path the file cannot hold, {} submodules not checked out\n",
        plan.restore.len(),
        plan.changed,
        plan.new,
        plan.deleted,
        plan.absent,
        plan.unsupported,
        plan.submodules
    ))
}

fn remove_restored(path: &Path) -> Result<(), CiError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(CiError::new(format!(
            "cannot remove {}: {error}",
            path.display()
        ))),
    }
}
