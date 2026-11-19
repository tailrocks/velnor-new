// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use super::input_target::target_object;
use crate::error::CiError;

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

pub(super) struct Pending {
    pub targets: Vec<PathBuf>,
    files: Vec<PathBuf>,
}

impl Pending {
    pub(super) fn consume(self) -> Result<(), CiError> {
        for file in self.files {
            match fs::remove_file(&file) {
                Ok(()) => {}
                Err(e) if e.kind() == ErrorKind::NotFound => {}
                Err(e) => return Err(failed("remove", &file, e)),
            }
        }
        Ok(())
    }
}

pub(super) fn record(root: &Path, target: &Path) -> Result<(), CiError> {
    let directory = directory(root)?;
    fs::create_dir_all(&directory).map_err(|e| failed("create", &directory, e))?;
    regular_directory(&directory)?;
    let target = target_object(&root.join(target))?;
    let name = filename(&target)?;
    let bytes =
        serde_json::to_vec(&target).map_err(|e| failed("encode", &directory.join(&name), e))?;
    let (staged, mut file) = loop {
        let sequence = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let staged = directory.join(format!(".{name}.{}.{sequence}.tmp", std::process::id()));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&staged)
        {
            Ok(file) => break (staged, file),
            Err(e) if e.kind() == ErrorKind::AlreadyExists => {}
            Err(e) => return Err(failed("create", &staged, e)),
        }
    };
    if let Err(e) = file.write_all(&bytes) {
        fs::remove_file(&staged).map_err(|e| failed("remove", &staged, e))?;
        return Err(failed("write", &staged, e));
    }
    drop(file);
    fs::rename(&staged, directory.join(name)).map_err(|e| failed("publish", &staged, e))
}

pub(super) fn read(root: &Path) -> Result<Pending, CiError> {
    let directory = directory(root)?;
    match fs::symlink_metadata(&directory) {
        Ok(_) => regular_directory(&directory)?,
        Err(e) if e.kind() == ErrorKind::NotFound => {
            return Ok(Pending {
                targets: Vec::new(),
                files: Vec::new(),
            });
        }
        Err(e) => return Err(failed("read", &directory, e)),
    }
    let mut pending = Pending {
        targets: Vec::new(),
        files: Vec::new(),
    };
    for entry in fs::read_dir(&directory).map_err(|e| failed("list", &directory, e))? {
        let path = entry.map_err(|e| failed("list", &directory, e))?.path();
        if path.extension().is_some_and(|extension| extension == "tmp") {
            continue;
        }
        let metadata = fs::symlink_metadata(&path).map_err(|e| failed("read", &path, e))?;
        if !metadata.is_file() {
            return Err(failed("read", &path, "expected a regular target receipt"));
        }
        let bytes = fs::read(&path).map_err(|e| failed("read", &path, e))?;
        let target: PathBuf =
            serde_json::from_slice(&bytes).map_err(|e| failed("parse", &path, e))?;
        if !target.is_absolute() || path.file_name() != Some(filename(&target)?.as_ref()) {
            return Err(failed(
                "read",
                &path,
                "target identity does not match its receipt",
            ));
        }
        pending.targets.push(target);
        pending.files.push(path);
    }
    Ok(pending)
}

fn directory(root: &Path) -> Result<PathBuf, CiError> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args([
            "rev-parse",
            "--path-format=absolute",
            "--git-path",
            "velnor-restored-targets",
        ])
        .output()
        .map_err(|e| failed("locate", root, e))?;
    if !output.status.success() {
        return Err(failed(
            "locate",
            root,
            String::from_utf8_lossy(&output.stderr).trim(),
        ));
    }
    let path = String::from_utf8(output.stdout).map_err(|e| failed("locate", root, e))?;
    Ok(PathBuf::from(path.trim_end_matches(['\n', '\r'])))
}

fn regular_directory(path: &Path) -> Result<(), CiError> {
    let metadata = fs::symlink_metadata(path).map_err(|e| failed("read", path, e))?;
    if metadata.is_dir() {
        Ok(())
    } else {
        Err(failed("read", path, "expected a regular receipt directory"))
    }
}

fn filename(target: &Path) -> Result<String, CiError> {
    let text = target
        .to_str()
        .ok_or_else(|| failed("encode", target, "target is not valid UTF-8"))?;
    Ok(format!("{}.json", velnor_rust::sha256_hex(text.as_bytes())))
}

fn failed(what: &str, path: &Path, error: impl std::fmt::Display) -> CiError {
    CiError::new(format!(
        "cannot {what} restored target receipt {}: {error}",
        path.display()
    ))
}
