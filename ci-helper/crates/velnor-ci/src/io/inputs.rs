// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs::{self, File, FileTimes};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::error::CiError;
use crate::inputs::{
    INPUTS_FILE, SavedInput, Tracked, TrackedFile, format_inputs, is_representable, parse_inputs,
    parse_ls_files, plan_restore, with_worktree_blobs,
};

pub fn save_inputs(root: &Path) -> Result<String, CiError> {
    let (top, tracked) = worktree_files(root)?;
    let path = root.join(INPUTS_FILE);
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
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| CiError::new(format!("cannot create {}: {e}", parent.display())))?;
    }
    fs::write(&path, format_inputs(&saved))
        .map_err(|e| CiError::new(format!("cannot write {}: {e}", path.display())))?;
    Ok(format!(
        "saved {} tracked files and {} symlinks to {INPUTS_FILE}; left out: {} not in the worktree, {} with a path the file cannot hold, {} submodules not checked out\n",
        tracked.files.len(),
        tracked.links.len(),
        tracked.absent.len(),
        tracked.unsupported,
        tracked.submodules
    ))
}

pub fn restore_inputs(root: &Path) -> Result<String, CiError> {
    let path = root.join(INPUTS_FILE);
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == ErrorKind::NotFound => {
            return Ok(format!("no {INPUTS_FILE}; nothing restored (cold run)\n"));
        }
        Err(e) => return Err(CiError::new(format!("cannot read {}: {e}", path.display()))),
    };
    let saved = parse_inputs(&text)?;
    let (top, tracked) = worktree_files(root)?;
    let plan = plan_restore(&saved, &tracked);
    if plan.links_changed > 0 {
        return Ok(format!(
            "restored nothing: {} tracked symlinks are new, changed or deleted since the save\n",
            plan.links_changed
        ));
    }
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

fn worktree_files(root: &Path) -> Result<(PathBuf, Tracked), CiError> {
    let top = git(root, &["rev-parse", "--show-toplevel"], None)?;
    let top = PathBuf::from(
        String::from_utf8(top)
            .map_err(|e| CiError::new(format!("git rev-parse printed non-UTF-8: {e}")))?
            .trim_end_matches(['\n', '\r']),
    );
    let tracked = top_level_files(&top)?;
    Ok((top, tracked))
}

fn top_level_files(root: &Path) -> Result<Tracked, CiError> {
    let index = parse_ls_files(&git(
        root,
        &["ls-files", "-s", "-z", "--recurse-submodules"],
        None,
    )?);
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

fn git(root: &Path, args: &[&str], stdin: Option<&[u8]>) -> Result<Vec<u8>, CiError> {
    let mut child = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| CiError::new(format!("cannot run git {}: {e}", args[0])))?;
    let input = child.stdin.take();
    let output = std::thread::scope(|scope| {
        let writer = scope.spawn(move || match (input, stdin) {
            (Some(mut pipe), Some(bytes)) => pipe.write_all(bytes),
            _ => Ok(()),
        });
        let output = child.wait_with_output();
        let written = writer
            .join()
            .unwrap_or_else(|_| Err(std::io::Error::other("stdin writer panicked")));
        output.and_then(|output| written.map(|()| output))
    })
    .map_err(|e| CiError::new(format!("git {}: {e}", args[0])))?;
    if !output.status.success() {
        return Err(CiError::new(format!(
            "git {} failed: {}",
            args[0],
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(output.stdout)
}

fn modified_ns(path: &Path) -> Result<u64, CiError> {
    let modified = fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .map_err(|e| CiError::new(format!("cannot read mtime of {}: {e}", path.display())))?;
    modified
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|since| u64::try_from(since.as_nanos()).ok())
        .ok_or_else(|| CiError::new(format!("mtime of {} is out of range", path.display())))
}

fn set_modified_ns(path: &Path, mtime_ns: u64) -> Result<(), CiError> {
    let time = SystemTime::UNIX_EPOCH + Duration::from_nanos(mtime_ns);
    File::options()
        .write(true)
        .open(path)
        .or_else(|_| File::open(path))
        .and_then(|file| file.set_times(FileTimes::new().set_modified(time)))
        .map_err(|e| CiError::new(format!("cannot set mtime of {}: {e}", path.display())))
}
