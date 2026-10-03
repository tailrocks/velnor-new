//! Portable Cargo scheduler state carried beside an exported action closure.

use crate::config::Config;
use eyre::{Context as _, Result, bail};
use mbx_cache_core::{CacheDigest, LocalCas};
use mbx_cache_store::{CargoBuildRoots, ExportAdditions, WorkspaceRoots};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{File, FileTimes};
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::UNIX_EPOCH;

pub(crate) const ATTACHMENT: &str = "cargo-workspace-state-v4";
const VERSION: u8 = 4;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Bundle {
    version: u8,
    workspaces: Vec<WorkspaceState>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkspaceState {
    owner: CacheDigest,
    workspace_root: PathBuf,
    cargo_roots: CargoBuildRoots,
    signature: CacheDigest,
    trees: Vec<RootTree>,
    owned_out_dirs: Vec<crate::out_dir::Snapshot>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum RootRole {
    Target,
    Build,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RootTree {
    role: RootRole,
    inline_archive: CacheDigest,
    inline_files: Vec<FileMetadata>,
    references: Vec<FileReference>,
    symlinks: Vec<Symlink>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileReference {
    path: PathBuf,
    source: FileSource,
    mode: u32,
    modified_secs: u64,
    modified_nanos: u32,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileMetadata {
    path: PathBuf,
    mode: u32,
    modified_secs: u64,
    modified_nanos: u32,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum FileSource {
    Cas(CacheDigest),
    Mbx,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Symlink {
    path: PathBuf,
    target: PathBuf,
    directory: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum RestoreOutcome {
    Restored { files: u64, referenced_bytes: u64 },
    SkippedUnavailable,
    SkippedIncompatible,
    SkippedAmbiguous,
    SkippedNonempty,
    SkippedManagedOverlap,
}

#[derive(Debug)]
pub(crate) enum CaptureOutcome {
    Captured(ExportAdditions),
    RetainedOwner {
        additions: ExportAdditions,
        reasons: Vec<String>,
    },
    UnavailableManagedOverlap,
    UnavailableOwnerProof {
        reason: String,
    },
}

pub(crate) struct RetainOutcome {
    pub additions: ExportAdditions,
    pub unavailable_reasons: Vec<String>,
}

struct OwnedView {
    link: PathBuf,
    target: PathBuf,
    record: PathBuf,
    record_bytes: Vec<u8>,
    record_metadata: std::fs::Metadata,
}

#[path = "workspace_state/capture.rs"]
mod capture;
use capture::resolve_roots;
pub(crate) use capture::{capture, retain};
#[path = "workspace_state/inventory.rs"]
mod inventory;
pub(crate) use inventory::{referenced_objects, semantic_inventory, validate_receipt_evidence};
#[path = "workspace_state/lineage.rs"]
mod lineage;
#[path = "workspace_state/lineage_capture.rs"]
mod lineage_capture;
#[path = "workspace_state/lineage_select.rs"]
mod lineage_select;
#[path = "workspace_state/owned_out_dirs.rs"]
mod owned_out_dirs;
#[path = "workspace_state/placement.rs"]
mod placement;
pub(crate) use lineage::freeze_lineage;
#[path = "workspace_state/useful.rs"]
mod useful;
#[path = "workspace_state/validation.rs"]
mod validation;
pub(crate) use validation::validate_semantic_inventory;
#[path = "workspace_state/semantic.rs"]
mod semantic;
use semantic::*;
#[path = "workspace_state/paths.rs"]
mod paths;
use paths::*;
#[path = "workspace_state/restore.rs"]
mod restore;
pub(crate) use restore::restore;
use restore::{physical_root, role_root, validate_root_relationship};
#[path = "workspace_state/files.rs"]
mod files;
use files::*;
#[path = "workspace_state/metadata.rs"]
mod metadata;
use metadata::*;

#[cfg(test)]
#[path = "workspace_state/tests.rs"]
mod tests;

#[cfg(test)]
#[path = "workspace_state/role_tests.rs"]
mod role_tests;

#[cfg(all(test, unix))]
#[path = "workspace_state/managed_tests.rs"]
mod managed_tests;

#[cfg(test)]
#[path = "workspace_state/nested_tests.rs"]
mod nested_tests;

#[cfg(all(test, unix))]
#[path = "workspace_state/managed_restore_tests.rs"]
mod managed_restore_tests;

#[cfg(test)]
#[path = "workspace_state/out_dir_tests.rs"]
mod out_dir_tests;

#[cfg(test)]
#[path = "workspace_state/placement_tests.rs"]
mod placement_tests;

#[cfg(test)]
#[path = "workspace_state/out_dir_cas_tests.rs"]
mod out_dir_cas_tests;
