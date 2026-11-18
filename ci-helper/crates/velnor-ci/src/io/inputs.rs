// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use super::input_scopes::{git_prefix, scoped_files};
pub(super) use super::inputs_git::{git, modified_ns, set_modified_ns};
use crate::error::CiError;
use crate::inputs::{
    SavedInput, Tracked, TrackedFile, format_inputs, inputs_file, is_representable, parse_inputs,
    parse_ls_files, plan_restore, snapshot_file, with_worktree_blobs, witness_file,
};
use crate::scope::Scope;

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
    let relative = target.join(inputs_file(&git_prefix(root)?));
    let path = root.join(&relative);
    check_snapshot_parent(&path)?;
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
    publish_snapshot(&path, &format_inputs(&saved))?;
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
    let saved =
        parse_inputs(&text).map_err(|e| CiError::new(format!("{}: {e}", path.display())))?;
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

pub(super) struct TrackedPaths {
    pub top: PathBuf,
    pub paths: Vec<PathBuf>,
}

pub(super) fn tracked_paths(root: &Path) -> Result<TrackedPaths, CiError> {
    let top = git(root, &["rev-parse", "--show-toplevel"], None)?;
    let top = PathBuf::from(
        String::from_utf8(top)
            .map_err(|e| CiError::new(format!("git rev-parse printed non-UTF-8: {e}")))?
            .trim_end_matches(['\n', '\r']),
    );
    let listed = git(&top, &["ls-files", "-z", "--recurse-submodules"], None)?;
    let mut paths = Vec::new();
    for path in listed
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
    {
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStringExt;
            paths.push(PathBuf::from(std::ffi::OsString::from_vec(path.to_vec())));
        }
        #[cfg(not(unix))]
        {
            paths.push(PathBuf::from(String::from_utf8(path.to_vec()).map_err(
                |e| CiError::new(format!("git ls-files printed non-UTF-8: {e}")),
            )?));
        }
    }
    Ok(TrackedPaths { top, paths })
}

pub(super) fn top_level_files(root: &Path, scope: &Scope) -> Result<Tracked, CiError> {
    let index = parse_ls_files(
        &git(
            root,
            &["ls-files", "-s", "-z", "--recurse-submodules"],
            None,
        )?,
        scope,
    );
    let mut present = Vec::with_capacity(index.files.len());
    let mut absent = Vec::new();
    for file in index.files {
        match fs::symlink_metadata(root.join(&file.path)) {
            Ok(metadata) if metadata.is_file() => present.push(TrackedFile {
                mode: worktree_mode(&metadata, &file.mode),
                ..file
            }),
            _ => absent.push(file.path),
        }
    }
    let mut paths = String::new();
    for file in &present {
        let path = root.join(&file.path);
        let path = path
            .to_str()
            .ok_or_else(|| CiError::new(format!("{} is not valid UTF-8", path.display())))?;
        paths.push_str(path);
        paths.push('\n');
    }
    let hashes = git(
        root,
        &["hash-object", "--no-filters", "--stdin-paths"],
        Some(paths.as_bytes()),
    )?;
    let hashes = String::from_utf8(hashes)
        .map_err(|e| CiError::new(format!("git hash-object printed non-UTF-8: {e}")))?;
    let mut links = Vec::with_capacity(index.links.len());
    let mut unverifiable_links = index.unverifiable_links;
    for link in index.links {
        let Ok(target) = fs::read_link(root.join(&link.path)) else {
            continue;
        };
        match target.to_str() {
            Some(text) if is_representable(text) => links.push(TrackedFile {
                blob: text.to_owned(),
                ..link
            }),
            _ => unverifiable_links += 1,
        }
    }
    Ok(Tracked {
        files: with_worktree_blobs(present, &hashes)?,
        links,
        absent,
        unsupported: index.unsupported,
        unverifiable_links,
        submodules: index.submodules,
    })
}

#[cfg(unix)]
fn worktree_mode(metadata: &fs::Metadata, _index_mode: &str) -> String {
    use std::os::unix::fs::PermissionsExt;
    format!("{:o}", 0o100_000 | (metadata.permissions().mode() & 0o7777))
}

#[cfg(not(unix))]
fn worktree_mode(_metadata: &fs::Metadata, index_mode: &str) -> String {
    index_mode.to_owned()
}
