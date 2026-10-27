// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[path = "cache/archive.rs"]
mod archive;
#[path = "cache/cleanup.rs"]
mod cleanup;
#[path = "cache/publish.rs"]
mod publish;
#[path = "cache/recover.rs"]
mod recover;
#[path = "cache/request.rs"]
mod request;

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use velnor_actions_orchestrator::EntryTupleV1;
use velnor_actions_orchestrator::InventoryV1;
use velnor_actions_orchestrator::cache_recovery::ObservedEntry;

use super::fail_internal;
use super::operation::InternalOp;

pub(super) const CACHE_PUBLISH_OP: &str = "cache-publish-v1";
pub(super) const CACHE_RECOVER_OP: &str = "cache-recover-v1";
pub(super) const CACHE_CLEANUP_OP: &str = "cache-cleanup-v1";

const CACHE_SCHEMA: u32 = 1;
const REQUEST_DIR: &str = "velnor";
const RUNNER_TEMP_ENV: &str = "RUNNER_TEMP";
const REQUEST_FILE_ENV: &str = "VELNOR_REQUEST_FILE";
const PUBLISH_REQUEST_FILE: &str = "cache-publish.json";
const RECOVER_REQUEST_FILE: &str = "cache-recover.json";
const CLEANUP_REQUEST_FILE: &str = "cache-cleanup.json";
const MAX_CACHE_REQUEST_BYTES: u64 = 64 * 1024;
const MAX_CLEANUP_CANDIDATES: usize = 8;

/// Fixed transport layout, mirroring the renderer's complete-state literals.
///
/// The renderer derives these paths from the contract
/// `COMPLETE_STATE_ROOT_EXPR`/`COMPLETE_STATE_TRANSPORT_EXPR` expressions;
/// the CLI cannot depend on the contract crate, so the runner-relative
/// suffixes are pinned here with a test that fails loudly on drift.
const STATE_ROOT_REL: &str = "velnor-gc1/state";
const STATE_TRANSPORT_REL: &str = "velnor-gc1/transport/state/payload.tar.gz";
const STATE_STAGE_REL: &str = "velnor/state-stage";
const STATE_BASE: &str = "state";

/// Cap for the number of archive members the publish and recover bodies
/// accept. Mirrors the orchestrator snapshot archive entry budget so a
/// hostile or degenerate payload is refused before extraction.
const MAX_ARCHIVE_MEMBERS: u64 = 100_000;

const COMMAND_KEYS: [&str; 8] = [
    "command",
    "argv",
    "shell",
    "exec",
    "script",
    "run",
    "entrypoint",
    "container",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CacheOp {
    Publish,
    Recover,
    Cleanup,
}

impl CacheOp {
    fn from_internal(op: InternalOp) -> Option<Self> {
        match op {
            InternalOp::CachePublish => Some(Self::Publish),
            InternalOp::CacheRecover => Some(Self::Recover),
            InternalOp::CacheCleanup => Some(Self::Cleanup),
            _ => None,
        }
    }

    fn operation_name(self) -> &'static str {
        match self {
            Self::Publish => "publish",
            Self::Recover => "recover",
            Self::Cleanup => "cleanup",
        }
    }

    fn request_file(self) -> &'static str {
        match self {
            Self::Publish => PUBLISH_REQUEST_FILE,
            Self::Recover => RECOVER_REQUEST_FILE,
            Self::Cleanup => CLEANUP_REQUEST_FILE,
        }
    }
}

struct ValidatedCacheRequest {
    admission_bytes: Vec<u8>,
    inventory_bytes: Vec<u8>,
    expected_bytes: Vec<u8>,
    candidate_ids: Vec<u64>,
}

struct Expected {
    key: String,
    ref_name: String,
    version: String,
}

pub(super) fn gate_request(op: InternalOp, path: &Path) -> Option<PathBuf> {
    let cache_op = CacheOp::from_internal(op)?;
    let runner_temp = env::var_os(RUNNER_TEMP_ENV)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)?;
    if !runner_temp.is_absolute() {
        return None;
    }
    let root = runner_temp.join(REQUEST_DIR);
    if path != root.join(cache_op.request_file()) {
        return None;
    }
    if env::var_os(REQUEST_FILE_ENV).as_deref() != Some(path.as_os_str()) {
        return None;
    }
    let root_meta = fs::symlink_metadata(&root).ok()?;
    if root_meta.file_type().is_symlink() || !root_meta.is_dir() {
        return None;
    }
    let request_meta = fs::symlink_metadata(path).ok()?;
    if request_meta.file_type().is_symlink() || !request_meta.is_file() {
        return None;
    }
    if request_meta.len() == 0 || request_meta.len() > MAX_CACHE_REQUEST_BYTES {
        return None;
    }
    Some(path.to_path_buf())
}

pub(super) fn run(op: InternalOp, path: &Path) -> ExitCode {
    let Some(cache_op) = CacheOp::from_internal(op) else {
        return fail_internal("cache_operation_malformed");
    };
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) => return fail_internal(&format!("read cache request: {error}")),
    };
    let request = match request::validate_request(cache_op, &bytes) {
        Ok(request) => request,
        Err(problem) => return fail_internal(&problem),
    };
    let Some(runner_temp) = runner_temp_dir() else {
        return fail_internal("missing runner temp");
    };
    match cache_op {
        CacheOp::Publish => publish::run_publish(&request, &runner_temp),
        CacheOp::Recover => recover::run_recover(&request, &runner_temp),
        CacheOp::Cleanup => cleanup::run_cleanup(&request.inventory_bytes, &request.candidate_ids),
    }
}

fn runner_temp_dir() -> Option<PathBuf> {
    let runner_temp = env::var_os(RUNNER_TEMP_ENV)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)?;
    if runner_temp.is_absolute() {
        Some(runner_temp)
    } else {
        None
    }
}

fn find_entry<'inventory>(
    inventory: &'inventory InventoryV1,
    expected: &Expected,
) -> Option<&'inventory EntryTupleV1> {
    // Match the complete (key, ref, version) tuple: the same logical key
    // may exist under several refs or cache versions, and key-only lookup
    // would select an unrelated entry and fail readback even when the
    // correct entry is present.
    inventory.entries.iter().find(|entry| {
        entry.key == expected.key
            && entry.ref_name == expected.ref_name
            && entry.version == expected.version
    })
}

fn observed_entry(entry: &EntryTupleV1) -> ObservedEntry {
    ObservedEntry {
        repository_id: entry.repository_id,
        cache_id: entry.cache_id,
        ref_name: entry.ref_name.clone(),
        key: entry.key.clone(),
        version: entry.version.clone(),
        size_in_bytes: entry.size_in_bytes,
        created_at: entry.created_at.clone(),
    }
}

fn single_line(text: &str) -> String {
    text.replace(['\n', '\r'], " ")
}
