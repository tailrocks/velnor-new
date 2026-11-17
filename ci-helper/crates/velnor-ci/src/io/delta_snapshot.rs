// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::delta::{failure, files, safe_path};
use super::inputs::modified_ns;
use crate::error::CiError;

pub(super) const BASELINE: &str = ".velnor/delta-baseline.json";
pub(super) const REMOVED: &str = "removed.json";
const METADATA_LIMIT: u64 = 16 * 1024 * 1024;

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Identity {
    pub size: u64,
    pub modified: u64,
    digest: [u8; 32],
}

#[derive(Serialize, Deserialize)]
pub(super) struct Snapshot {
    pub base: String,
    pub files: BTreeMap<PathBuf, Identity>,
}

pub(super) fn capture(root: &Path, base: &str) -> Result<Snapshot, CiError> {
    let files = files(root)?
        .into_iter()
        .map(|path| identity(&root.join(&path)).map(|identity| (path, identity)))
        .collect::<Result<_, _>>()?;
    Ok(Snapshot {
        base: base.to_owned(),
        files,
    })
}

pub(super) fn identity(path: &Path) -> Result<Identity, CiError> {
    safe_path(path)?;
    let mut file = fs::File::open(path).map_err(|e| failure(path, &e))?;
    let size = file.metadata().map_err(|e| failure(path, &e))?.len();
    let mut digest = Sha256::new();
    let mut buffer = vec![0_u8; 64 * 1024].into_boxed_slice();
    loop {
        let count = file.read(&mut buffer).map_err(|e| failure(path, &e))?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(Identity {
        size,
        modified: modified_ns(path)?,
        digest: digest.finalize().into(),
    })
}

pub(super) fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>, CiError> {
    let bytes = serde_json::to_vec(value).map_err(|e| CiError::new(e.to_string()))?;
    if bytes.len() as u64 > METADATA_LIMIT {
        return Err(CiError::new("delta metadata exceeds its size limit"));
    }
    Ok(bytes)
}

pub(super) fn read<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, CiError> {
    safe_path(path)?;
    let metadata = fs::symlink_metadata(path).map_err(|e| failure(path, &e))?;
    if !metadata.is_file() || metadata.len() > METADATA_LIMIT {
        return Err(CiError::new(
            "delta metadata must be a bounded regular file",
        ));
    }
    let file = fs::File::open(path).map_err(|e| failure(path, &e))?;
    serde_json::from_slice(&super::read_limited(file, METADATA_LIMIT)?)
        .map_err(|e| CiError::new(format!("invalid delta metadata: {e}")))
}

pub(super) fn replace(path: &Path, bytes: Option<&[u8]>) -> Result<(), CiError> {
    if let Some(parent) = path.parent() {
        safe_path(parent)?;
        fs::create_dir_all(parent).map_err(|e| failure(parent, &e))?;
    }
    match fs::remove_file(path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(failure(path, &error)),
    }
    if let Some(bytes) = bytes {
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .and_then(|mut file| file.write_all(bytes))
            .map_err(|e| failure(path, &e))?;
    }
    Ok(())
}

pub(super) fn validate(path: &Path) -> Result<(), CiError> {
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
        || (path.starts_with(".velnor") && path != Path::new(".velnor/inputs.tsv"))
    {
        return Err(CiError::new(format!(
            "invalid delta relative path {}",
            path.display()
        )));
    }
    Ok(())
}

pub(super) fn apply(staging: &Path, target: &Path) -> Result<usize, CiError> {
    let removed: Vec<PathBuf> = read(&staging.join(REMOVED))?;
    let payload = staging.join("files");
    let copied = files(&payload)?;
    for path in removed.iter().chain(&copied) {
        validate(path)?;
        safe_path(&target.join(path))?;
    }
    for path in &removed {
        let destination = target.join(path);
        match fs::remove_file(&destination) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(failure(&destination, &e)),
        }
    }
    for path in &copied {
        super::delta::copy(&payload.join(path), &target.join(path))?;
    }
    Ok(copied.len())
}
