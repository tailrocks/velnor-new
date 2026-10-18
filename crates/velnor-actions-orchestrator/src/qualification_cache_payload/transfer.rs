//! Write one typed cache payload to a staging directory.
//! Check the manifest and cache receipt before writing payload bytes into live roots.

use super::roots::CacheRootId;
pub(crate) use super::{CacheExportProof, CacheRestoreProof, CacheRestoreStatus};
use std::io::{Read, Seek};

#[path = "transfer_cleanup.rs"]
pub(super) mod cleanup;
#[path = "transfer_dirs.rs"]
pub(super) mod dirs;
#[path = "transfer_engine.rs"]
pub(super) mod engine;
#[path = "transfer_files.rs"]
pub(super) mod files;
pub(super) use files::open_copy_source;
#[path = "transfer_install.rs"]
pub(super) mod install;
#[path = "payload.rs"]
pub(super) mod payload;
#[path = "transfer_source.rs"]
pub(super) mod source;
#[path = "transfer_stage.rs"]
pub(super) mod stage;
pub(super) use dirs::same_version;
#[path = "transfer_transport_envelope.rs"]
pub(super) mod transport_envelope;
#[path = "transfer_transport_tar_write.rs"]
pub(super) mod transport_tar_write;
pub(crate) use engine::{snapshot_roots, useful_state_digest};
pub(crate) use transport_tar_write::write_payload as write_transport_payload;

pub(crate) fn validate_transport_envelope<R: Read + Seek>(
    reader: &mut R,
    operation_limit: u64,
) -> Result<(), &'static str> {
    let range = transport_envelope::read_envelope(reader, operation_limit)?;
    if range
        .offset
        .checked_add(range.length)
        .is_none_or(|end| end > operation_limit)
    {
        return Err("qualification_cache_transport_length_invalid");
    }
    Ok(())
}

const MANIFEST_FILENAME: &str = "manifest.json";
const ROOTS_DIRECTORY: &str = "roots";
const STATE_DOMAIN: &[u8] = b"velnor-cache-useful-state\0";
const FILE_DOMAIN: &[u8] = b"velnor-cache-payload-file-v1\0";
const MAX_MANIFEST_BYTES: usize = 65_536;
const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024 * 1024;
const MAX_DEPTH: usize = 128;
const MAX_ENTRIES: usize = 250_000;

#[derive(Debug)]
pub(super) struct CreatedPayload {
    pub(super) digest: String,
    pub(super) descriptor: rustix::fd::OwnedFd,
    pub(super) stat: rustix::fs::Stat,
}

#[cfg(test)]
pub(crate) use engine::{export_payload, import_payload, snapshot_root};

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
#[path = "tests/dispatch_cache_transfer_tests.rs"]
mod tests;
