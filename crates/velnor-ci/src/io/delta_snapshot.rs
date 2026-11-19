// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::delta::{failure, safe_path};
use super::inputs::modified_ns;
use crate::error::CiError;

pub(super) const BASELINE: &str = ".velnor/delta-baseline.json";
pub(super) const MANIFEST: &str = "manifest.json";
/// The base inventory a default-branch run saves inside the base cache.
pub(super) const INVENTORY: &str = ".velnor/delta-inventory.json";
const METADATA_LIMIT: u64 = 16 * 1024 * 1024;
const INVENTORY_LIMIT: u64 = 128 * 1024 * 1024;
/// A file changed within this long before an inventory was taken is read again: a coarse
/// file-system clock can give a later rewrite the same change time.
const RACY_NS: u64 = 2_000_000_000;

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
    pub(super) fn validate_time(&self, cutoff: u64) -> Result<(), CiError> {
        if self.metadata().0 > cutoff {
            return Err(CiError::new(
                "cached build modification time is in the future",
            ));
        }
        Ok(())
    }
}

pub(super) fn now_ns() -> Result<u64, CiError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|elapsed| u64::try_from(elapsed.as_nanos()).ok())
        .ok_or_else(|| CiError::new("current modification time is out of range"))
}

pub(super) fn validate_times(nodes: &BTreeMap<PathBuf, Node>, cutoff: u64) -> Result<(), CiError> {
    nodes
        .values()
        .try_for_each(|node| node.validate_time(cutoff))
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Snapshot {
    pub base: String,
    pub nodes: BTreeMap<PathBuf, Node>,
    /// Inode change time of each file when it was inventoried. A rewrite always moves it, even
    /// one that restores the size and modification time, so a file whose change time is
    /// unchanged still holds the recorded bytes.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub changed: BTreeMap<PathBuf, u64>,
    /// When the inventory began; change times closer to it than `RACY_NS` prove nothing.
    #[serde(default)]
    pub taken: u64,
}

impl Snapshot {
    pub(super) fn files(&self) -> usize {
        self.nodes.values().filter(|node| !node.directory()).count()
    }

    /// The recorded node of a file that provably still holds the recorded bytes, so the
    /// caller need not read it: size, time, mode and change time all match, and the change
    /// time is old enough that a later rewrite could not share it.
    pub(super) fn unchanged(&self, path: &Path, current: &Node, changed: u64) -> Option<&Node> {
        let recorded = self.nodes.get(path)?;
        let same = matches!((recorded, current), (Node::File { .. }, Node::File { .. }))
            && recorded.size() == current.size()
            && recorded.metadata() == current.metadata();
        let at = *self.changed.get(path)?;
        (same && at == changed && at.saturating_add(RACY_NS) <= self.taken).then_some(recorded)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Manifest {
    pub base: String,
    pub nodes: BTreeMap<PathBuf, Node>,
    pub removed: Vec<PathBuf>,
    pub sources: super::delta_source::SourceSnapshot,
}

/// Inventories `root`, started at `taken`. A file whose size, time and mode match the same
/// path in `known` keeps its recorded digest instead of being read again; any other file is
/// hashed. Also returns how many files were read.
pub(super) fn capture(
    root: &Path,
    base: &str,
    known: Option<&Snapshot>,
    taken: u64,
) -> Result<(Snapshot, usize), CiError> {
    let mut snapshot = Snapshot {
        base: base.to_owned(),
        nodes: BTreeMap::new(),
        changed: BTreeMap::new(),
        taken,
    };
    let mut read = 0;
    for path in super::delta_tree::paths(root)? {
        let full = root.join(&path);
        let changed = change_time(&full)?;
        let recorded = known.and_then(|known| known.nodes.get(&path));
        let (node, hashed) = identity_reusing(&full, recorded)?;
        read += usize::from(hashed);
        if !node.directory() {
            snapshot.changed.insert(path.clone(), changed);
        }
        snapshot.nodes.insert(path, node);
    }
    Ok((snapshot, read))
}

/// The inode change time of `path`, which a user cannot set.
pub(super) fn change_time(path: &Path) -> Result<u64, CiError> {
    let metadata = fs::symlink_metadata(path).map_err(|e| failure(path, &e))?;
    u64::try_from(i128::from(metadata.ctime()) * 1_000_000_000 + i128::from(metadata.ctime_nsec()))
        .map_err(|_| CiError::new("change time is out of range"))
}

fn identity_reusing(path: &Path, recorded: Option<&Node>) -> Result<(Node, bool), CiError> {
    let current = stat(path)?;
    if let (
        Node::File {
            size,
            modified,
            mode,
            ..
        },
        Some(Node::File {
            size: old_size,
            modified: old_modified,
            mode: old_mode,
            digest,
        }),
    ) = (&current, recorded)
        && (size, modified, mode) == (old_size, old_modified, old_mode)
    {
        return Ok((
            Node::File {
                size: *size,
                modified: *modified,
                mode: *mode,
                digest: *digest,
            },
            false,
        ));
    }
    Ok((identity(path)?, !current.directory()))
}

/// Records the digest of every file of a base that is about to be saved, so a pull request
/// that restores the base does not read the whole tree again before its first Cargo command.
pub(super) fn record_inventory(target: &Path) -> Result<(), CiError> {
    let (mut inventory, _) = capture(target, "", None, 0)?;
    inventory.changed.clear();
    let bytes = serde_json::to_vec(&inventory).map_err(|e| CiError::new(e.to_string()))?;
    if bytes.len() as u64 > INVENTORY_LIMIT {
        return Err(CiError::new("the base inventory exceeds its size limit"));
    }
    replace(&target.join(INVENTORY), Some(&bytes))
}

/// The inventory saved with the base, or `None` when it is absent or unusable; the caller
/// then reads every file.
pub(super) fn recorded_inventory(target: &Path) -> Option<Snapshot> {
    read_bounded(&target.join(INVENTORY), INVENTORY_LIMIT).ok()
}

pub(super) fn forget_inventory(target: &Path) -> Result<(), CiError> {
    replace(&target.join(INVENTORY), None)
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
    read_bounded(path, METADATA_LIMIT)
}

fn read_bounded<T: for<'de> Deserialize<'de>>(path: &Path, limit: u64) -> Result<T, CiError> {
    safe_path(path)?;
    let metadata = fs::symlink_metadata(path).map_err(|e| failure(path, &e))?;
    if !metadata.is_file() || metadata.len() > limit {
        return Err(CiError::new(
            "delta metadata must be a bounded regular file",
        ));
    }
    let file = fs::File::open(path).map_err(|e| failure(path, &e))?;
    serde_json::from_slice(&super::read_limited(file, limit)?)
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
        || (path.starts_with(".velnor") && !crate::inputs::is_inputs_file(path))
    {
        return Err(CiError::new(format!(
            "invalid delta relative path {}",
            path.display()
        )));
    }
    Ok(())
}
