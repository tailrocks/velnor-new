// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs::{self, File, FileTimes};
use std::io::ErrorKind;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::error::CiError;
use crate::inputs::{
    INPUTS_FILE, SavedInput, TrackedFile, format_inputs, parse_inputs, parse_ls_files, plan_restore,
};

pub fn save_inputs(root: &Path) -> Result<String, CiError> {
    let tracked = tracked_files(root)?;
    let mut saved = Vec::with_capacity(tracked.len());
    for file in &tracked {
        if let Some(mtime_ns) = modified_ns(&root.join(&file.path))? {
            saved.push(SavedInput {
                path: file.path.clone(),
                blob: file.blob.clone(),
                mtime_ns,
            });
        }
    }
    let path = root.join(INPUTS_FILE);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| CiError::new(format!("cannot create {}: {e}", parent.display())))?;
    }
    fs::write(&path, format_inputs(&saved))
        .map_err(|e| CiError::new(format!("cannot write {}: {e}", path.display())))?;
    Ok(format!(
        "saved {} tracked files to {INPUTS_FILE}\n",
        saved.len()
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
    let plan = plan_restore(&parse_inputs(&text)?, &tracked_files(root)?);
    for input in &plan.restore {
        set_modified_ns(&root.join(&input.path), input.mtime_ns)?;
    }
    Ok(format!(
        "restored the modification time of {} unchanged files; {} changed, {} new\n",
        plan.restore.len(),
        plan.changed,
        plan.new
    ))
}

fn tracked_files(root: &Path) -> Result<Vec<TrackedFile>, CiError> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["ls-files", "-s", "-z"])
        .output()
        .map_err(|e| CiError::new(format!("cannot run git ls-files: {e}")))?;
    if !output.status.success() {
        return Err(CiError::new(format!(
            "git ls-files failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(parse_ls_files(&output.stdout))
}

fn modified_ns(path: &Path) -> Result<Option<u64>, CiError> {
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(CiError::new(format!("cannot stat {}: {e}", path.display()))),
    };
    let modified = metadata
        .modified()
        .map_err(|e| CiError::new(format!("cannot read mtime of {}: {e}", path.display())))?;
    Ok(modified
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|since| u64::try_from(since.as_nanos()).ok()))
}

fn set_modified_ns(path: &Path, mtime_ns: u64) -> Result<(), CiError> {
    let time = SystemTime::UNIX_EPOCH + Duration::from_nanos(mtime_ns);
    File::open(path)
        .and_then(|file| file.set_times(FileTimes::new().set_modified(time)))
        .map_err(|e| CiError::new(format!("cannot set mtime of {}: {e}", path.display())))
}
