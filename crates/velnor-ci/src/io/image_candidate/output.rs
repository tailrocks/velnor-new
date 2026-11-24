// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use super::fail;
use crate::error::CiError;

const ATTEMPTS: u32 = 100;
const STAGING_PREFIX: &str = ".velnor-job-image-candidate-";

#[derive(Debug)]
pub(super) struct Outputs {
    pub(super) archive: PathBuf,
    pub(super) record: PathBuf,
}

fn symlinked(path: &Path) -> bool {
    path.ancestors().any(|component| {
        fs::symlink_metadata(component).is_ok_and(|meta| meta.file_type().is_symlink())
            && (component == path || !component.exists())
    })
}

fn resolve(path: &Path) -> PathBuf {
    let mut tail = Vec::new();
    let mut base = path;
    let resolved = loop {
        if let Ok(found) = fs::canonicalize(base) {
            break found;
        }
        match (base.parent(), base.file_name()) {
            (Some(parent), Some(name)) => {
                tail.push(name);
                base = parent;
            }
            _ => break base.to_path_buf(),
        }
    };
    tail.iter().rev().fold(resolved, |mut out, name| {
        out.push(name);
        out
    })
}

fn lexical(path: &Path) -> PathBuf {
    path.components().fold(PathBuf::new(), |mut out, part| {
        match part {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other),
        }
        out
    })
}

fn locate(root: &Path, raw: &Path) -> Result<PathBuf, CiError> {
    let requested = if raw.is_absolute() {
        raw.to_path_buf()
    } else {
        root.join(raw)
    };
    if symlinked(&requested) {
        return fail("Candidate output paths cannot use symlinks.");
    }
    let resolved = lexical(&resolve(&requested));
    if resolved.starts_with(root) {
        return fail("Candidate outputs must stay outside the source tree.");
    }
    Ok(resolved)
}

pub(super) fn plan(root: &Path, archive: &Path, record: &Path) -> Result<Outputs, CiError> {
    let outputs = Outputs {
        archive: locate(root, archive)?,
        record: locate(root, record)?,
    };
    if outputs.archive == outputs.record || outputs.archive.parent() != outputs.record.parent() {
        return fail("Candidate outputs must use separate files in one directory.");
    }
    if !outputs.archive.parent().is_some_and(Path::is_dir) {
        return fail("Candidate output directory must exist.");
    }
    if outputs.archive.exists() || outputs.record.exists() {
        return fail("Candidate outputs must be new, separate files.");
    }
    Ok(outputs)
}

#[derive(Debug)]
pub(super) struct Staging(PathBuf);

impl Staging {
    pub(super) fn create(parent: &Path) -> Result<Self, CiError> {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_nanos());
        for attempt in 0..ATTEMPTS {
            let path = parent.join(format!(
                "{STAGING_PREFIX}{}-{nanos}-{attempt}",
                std::process::id()
            ));
            match fs::create_dir(&path) {
                Ok(()) => return Ok(Self(path)),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return fail(format!("{}: {error}", path.display())),
            }
        }
        fail(format!(
            "cannot create a staging directory under {}",
            parent.display()
        ))
    }

    pub(super) fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Staging {
    fn drop(&mut self) {
        drop(fs::remove_dir_all(&self.0));
    }
}

pub(super) fn write_private(path: &Path, data: &[u8]) -> Result<(), CiError> {
    let failed = |error: io::Error| CiError::new(format!("{}: {error}", path.display()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .map_err(failed)?;
    file.write_all(data).map_err(failed)?;
    file.sync_all().map_err(failed)
}

fn same_file(left: &Path, right: &Path) -> bool {
    match (fs::metadata(left), fs::metadata(right)) {
        (Ok(a), Ok(b)) => a.dev() == b.dev() && a.ino() == b.ino(),
        _ => false,
    }
}

pub(super) fn publish_pair(staged: [(&Path, &Path); 2]) -> Result<(), CiError> {
    let mut linked: Vec<(&Path, &Path)> = Vec::new();
    for (source, destination) in staged {
        if let Err(error) = fs::hard_link(source, destination) {
            for (done_source, done_destination) in linked.into_iter().rev() {
                if same_file(done_source, done_destination) {
                    drop(fs::remove_file(done_destination));
                }
            }
            return fail(format!("{}: {error}", destination.display()));
        }
        linked.push((source, destination));
    }
    Ok(())
}
