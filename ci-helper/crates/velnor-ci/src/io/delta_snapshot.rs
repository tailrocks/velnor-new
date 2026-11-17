// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::delta::{failure, safe_path};
use super::inputs::modified_ns;
use crate::error::CiError;

pub(super) const BASELINE: &str = ".velnor/delta-baseline.json";
pub(super) const MANIFEST: &str = "manifest.json";
const METADATA_LIMIT: u64 = 16 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub(super) enum Node {
    File {
        size: u64,
        modified: u64,
        mode: u32,
        digest: [u8; 32],
    },
    Directory {
        modified: u64,
        mode: u32,
    },
}

impl Node {
    pub(super) fn directory(&self) -> bool {
        matches!(self, Self::Directory { .. })
    }
    pub(super) fn size(&self) -> u64 {
        match self {
            Self::File { size, .. } => *size,
            Self::Directory { .. } => 0,
        }
    }
    pub(super) fn metadata(&self) -> (u64, u32) {
        match self {
            Self::File { modified, mode, .. } | Self::Directory { modified, mode } => {
                (*modified, *mode)
            }
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Snapshot {
    pub base: String,
    pub nodes: BTreeMap<PathBuf, Node>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Manifest {
    pub base: String,
    pub nodes: BTreeMap<PathBuf, Node>,
    pub removed: Vec<PathBuf>,
}

pub(super) fn capture(root: &Path, base: &str) -> Result<Snapshot, CiError> {
    let nodes = super::delta_tree::paths(root)?
        .into_iter()
        .map(|path| identity(&root.join(&path)).map(|node| (path, node)))
        .collect::<Result<_, _>>()?;
    Ok(Snapshot {
        base: base.to_owned(),
        nodes,
    })
}

pub(super) fn identity(path: &Path) -> Result<Node, CiError> {
    let node = stat(path)?;
    let Node::File {
        size,
        modified,
        mode,
        ..
    } = node
    else {
        return Ok(node);
    };
    let mut file = fs::File::open(path).map_err(|e| failure(path, &e))?;
    let mut digest = Sha256::new();
    let mut buffer = vec![0_u8; 64 * 1024].into_boxed_slice();
    loop {
        let count = file.read(&mut buffer).map_err(|e| failure(path, &e))?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(Node::File {
        size,
        modified,
        mode,
        digest: digest.finalize().into(),
    })
}

pub(super) fn stat(path: &Path) -> Result<Node, CiError> {
    safe_path(path)?;
    let metadata = fs::symlink_metadata(path).map_err(|e| failure(path, &e))?;
    let modified = modified_ns(path)?;
    let mode = metadata.permissions().mode() & 0o7777;
    if metadata.is_dir() {
        return Ok(Node::Directory { modified, mode });
    }
    if !metadata.is_file() {
        return Err(CiError::new("unsupported delta node"));
    }
    Ok(Node::File {
        size: metadata.len(),
        modified,
        mode,
        digest: [0; 32],
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
    super::delta_tree::remove_node(path)?;
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
