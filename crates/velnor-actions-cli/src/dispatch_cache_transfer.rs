//! Snapshot and import one typed cache payload without exposing live roots to extraction.

use std::path::PathBuf;

use super::roots::CacheRootId;

#[path = "dispatch_cache_transfer_cleanup.rs"]
pub(super) mod cleanup;
#[path = "dispatch_cache_transfer_dirs.rs"]
pub(super) mod dirs;
#[path = "dispatch_cache_transfer_engine.rs"]
pub(super) mod engine;
#[path = "dispatch_cache_transfer_files.rs"]
pub(super) mod files;
#[path = "dispatch_cache_transfer_install.rs"]
pub(super) mod install;
#[path = "dispatch_cache_transfer_operation.rs"]
pub(super) mod operation;
#[path = "dispatch_cache_payload.rs"]
pub(super) mod payload;
#[path = "dispatch_cache_transfer_stage.rs"]
pub(super) mod stage;

const MANIFEST_FILENAME: &str = "manifest.json";
const ROOTS_DIRECTORY: &str = "roots";
const STATE_DOMAIN: &[u8] = b"velnor-cache-useful-state-v2\0";
const FILE_DOMAIN: &[u8] = b"velnor-cache-payload-file-v1\0";
const MAX_MANIFEST_BYTES: usize = 65_536;
const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024 * 1024;
const MAX_DEPTH: usize = 128;
const MAX_ENTRIES: usize = 250_000;
const ACTION_HIT_ENV: &str = "VELNOR_QUALIFICATION_CACHE_HIT";
const ACTION_MATCHED_KEY_ENV: &str = "VELNOR_QUALIFICATION_CACHE_MATCHED_KEY";
const MAX_ACTION_KEY_BYTES: usize = velnor_actions_contract::cachekey::MAX_CACHE_KEY_BYTES;

pub(crate) const EXPORT_OP: &str = "export-qualification-cache-payload-v1";
pub(crate) const IMPORT_OP: &str = "import-qualification-cache-payload-v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CacheExportProof {
    pub(crate) state_digest: String,
    pub(crate) save_key: Option<String>,
    pub(crate) save_stage_path: Option<PathBuf>,
    pub(crate) manifest_digest: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CacheRestoreStatus {
    Disabled,
    Miss,
    Hit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CacheRestoreProof {
    pub(crate) status: CacheRestoreStatus,
    pub(crate) state_digest: Option<String>,
    pub(crate) stage_path: Option<PathBuf>,
}

#[cfg(test)]
pub(crate) use engine::{export_payload, import_payload, snapshot_root, useful_state_digest};
pub(crate) use operation::{run_export, run_import};

pub(super) fn root_name(id: CacheRootId) -> &'static str {
    match id {
        CacheRootId::MbxActions => "mbx_actions",
        CacheRootId::CargoGitDb => "cargo_git_db",
        CacheRootId::CargoRegistryCache => "cargo_registry_cache",
        CacheRootId::CargoRegistryIndex => "cargo_registry_index",
        CacheRootId::CargoBin => "cargo_bin",
        CacheRootId::CargoCrates2Json => "cargo_crates2_json",
        CacheRootId::CargoCratesToml => "cargo_crates_toml",
        CacheRootId::MiseData => "mise_data",
        CacheRootId::RustupHome => "rustup_home",
        CacheRootId::TofuPlugins => "tofu_plugins",
        CacheRootId::TaskArtifacts => "task_artifacts",
    }
}

pub(super) fn is_file_root(id: CacheRootId) -> bool {
    matches!(
        id,
        CacheRootId::CargoCrates2Json | CacheRootId::CargoCratesToml
    )
}

pub(super) fn digest_bytes(value: &str) -> String {
    format!("b3-{}", blake3::hash(value.as_bytes()).to_hex())
}

pub(super) fn update_bytes(hasher: &mut blake3::Hasher, value: &[u8]) {
    hasher.update(&(value.len() as u64).to_be_bytes());
    hasher.update(value);
}

#[cfg(test)]
#[path = "dispatch_cache_transfer_tests.rs"]
mod tests;
