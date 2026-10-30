//! Verify the retained host storage authority for one runner job.

#[cfg(any(target_os = "linux", test))]
use super::storage_admission_types::StorageCacheAdmissionV1;
use std::path::Path;
#[cfg(any(target_os = "linux", test))]
use validation::validate_document;
#[cfg(test)]
use validation::{canonical_instance_binding_digest, canonical_projection_digest};

#[cfg(any(target_os = "linux", test))]
const MAX_DOCUMENT_BYTES: usize = 64 * 1024;
const MAX_OPERATION_BYTES: u64 = 4 * 1024 * 1024 * 1024;
const MAX_TRANSPORT_BYTES: u64 = 20 * 1024 * 1024 * 1024;
#[cfg(any(target_os = "linux", test))]
const ROLE_NAMES: &[&str] = &[
    "runner-root",
    "runner-home",
    "runner-work",
    "runner-tmp",
    "runner-shm",
    "shared-run",
    "dind-root",
    "dind-data",
    "dind-tmp",
    "dind-shm",
    "cache-live",
    "cache-stage",
    "engine-metadata",
];

/// Image identity from the controller-mounted, read-only runtime identity file.
#[cfg(any(target_os = "linux", test))]
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct RuntimeIdentityV1 {
    pub(super) domain: String,
    pub(super) schema_version: u32,
    pub(super) runner_image_name: String,
    pub(super) runner_source_revision: String,
    pub(super) runner_manifest_digest: String,
    pub(super) runner_config_digest: String,
    pub(super) platform: String,
}

/// Opaque byte and inode authority from the immutable, read-only job facts.
#[derive(Debug, Clone)]
pub(super) struct VerifiedCacheBudget {
    live_bytes: u64,
    live_inodes: u64,
    stage_bytes: u64,
    stage_inodes: u64,
    #[cfg(target_os = "linux")]
    runner_work: RunnerWorkFilesystem,
}

#[cfg(target_os = "linux")]
#[derive(Debug, Clone, Copy)]
pub(super) struct RunnerWorkFilesystem {
    mount_id: u64,
    device_major: u32,
    device_minor: u32,
    statx_device_major: u32,
    statx_device_minor: u32,
    fs_type_magic: u64,
    filesystem_id: u64,
}

impl VerifiedCacheBudget {
    /// Bound an import read by its request and remaining live-cache allowance.
    pub(super) fn operation_limit(
        &self,
        requested: u64,
        current_live_bytes: u64,
        current_live_inodes: u64,
        current_stage_bytes: u64,
        current_stage_inodes: u64,
    ) -> Result<u64, &'static str> {
        if current_live_bytes > self.live_bytes {
            return Err("cache_storage_live_budget_exceeded");
        }
        if current_live_inodes > self.live_inodes {
            return Err("cache_storage_inode_budget_exceeded");
        }
        if current_stage_bytes > self.stage_bytes {
            return Err("cache_storage_stage_budget_exceeded");
        }
        if current_stage_inodes > self.stage_inodes {
            return Err("cache_storage_inode_budget_exceeded");
        }
        let live_remaining = self.live_bytes - current_live_bytes;
        let stage_remaining = self.stage_bytes - current_stage_bytes;
        let effective = requested
            .min(MAX_OPERATION_BYTES)
            .min(MAX_TRANSPORT_BYTES)
            .min(live_remaining)
            .min(stage_remaining);
        if effective < 1_058 {
            return Err("cache_storage_byte_budget_exceeded");
        }
        Ok(effective)
    }

    pub(super) fn validate_import_growth(
        &self,
        current_live_bytes: u64,
        current_live_inodes: u64,
        growth_bytes: u64,
        growth_inodes: u64,
        available_bytes: u64,
        available_inodes: Option<u64>,
    ) -> Result<(), &'static str> {
        let total_bytes = current_live_bytes
            .checked_add(growth_bytes)
            .ok_or("cache_storage_live_budget_exceeded")?;
        let total_inodes = current_live_inodes
            .checked_add(growth_inodes)
            .ok_or("cache_storage_inode_budget_exceeded")?;
        if total_bytes > self.live_bytes {
            return Err("cache_storage_live_budget_exceeded");
        }
        if total_inodes > self.live_inodes {
            return Err("cache_storage_inode_budget_exceeded");
        }
        if growth_bytes > available_bytes
            || available_inodes.is_some_and(|available| growth_inodes > available)
        {
            return Err("cache_storage_filesystem_capacity_insufficient");
        }
        Ok(())
    }

    #[cfg(target_os = "linux")]
    pub(super) const fn runner_work(&self) -> RunnerWorkFilesystem {
        self.runner_work
    }

    #[cfg(test)]
    pub(super) const fn limits(&self) -> (u64, u64, u64, u64) {
        (
            self.live_bytes,
            self.live_inodes,
            self.stage_bytes,
            self.stage_inodes,
        )
    }
}

/// Parse and verify exact canonical host-projection bytes.
#[cfg(test)]
pub(super) fn parse_projection(
    bytes: &[u8],
    runner_temp: &Path,
    runner_name: &str,
) -> Result<VerifiedCacheBudget, &'static str> {
    if bytes.is_empty() || bytes.len() > MAX_DOCUMENT_BYTES {
        return Err("cache_storage_projection_size_invalid");
    }
    let document: StorageCacheAdmissionV1 =
        serde_json::from_slice(bytes).map_err(|_| "cache_storage_projection_invalid")?;
    if serde_json::to_vec(&document).map_err(|_| "cache_storage_projection_invalid")? != bytes {
        return Err("cache_storage_projection_noncanonical");
    }
    validate_document(&document, runner_temp, runner_name)?;
    Ok(budget_from_document(&document))
}

/// Read the fixed, controller-mounted projection and verify its observed mount.
pub(super) fn read_verified_budget(
    runner_temp: &Path,
    runner_name: &str,
) -> Result<VerifiedCacheBudget, &'static str> {
    #[cfg(target_os = "linux")]
    {
        let runtime_identity =
            identity::read_runtime_identity()?.ok_or("cache_storage_runtime_identity_missing")?;
        let (bytes, mount) = mount::read_projection()?;
        if bytes.is_empty() || bytes.len() > MAX_DOCUMENT_BYTES {
            return Err("cache_storage_projection_size_invalid");
        }
        let document: StorageCacheAdmissionV1 =
            serde_json::from_slice(&bytes).map_err(|_| "cache_storage_projection_invalid")?;
        if serde_json::to_vec(&document).map_err(|_| "cache_storage_projection_invalid")? != bytes {
            return Err("cache_storage_projection_noncanonical");
        }
        validate_document(&document, runner_temp, runner_name)?;
        if document.runner_identity != runtime_identity {
            return Err("cache_storage_runtime_identity_mismatch");
        }
        if !mount.agrees_with(&document.job_facts_mount) {
            return Err("cache_storage_mount_identity_mismatch");
        }
        Ok(budget_from_document(&document))
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (runner_temp, runner_name);
        Err("cache_storage_mount_unavailable")
    }
}

#[cfg(any(target_os = "linux", test))]
fn budget_from_document(document: &StorageCacheAdmissionV1) -> VerifiedCacheBudget {
    #[cfg(target_os = "linux")]
    let runner_work = {
        let work = &document.instance_binding.roles[2];
        RunnerWorkFilesystem {
            mount_id: work.mount_id,
            device_major: work.device_major,
            device_minor: work.device_minor,
            statx_device_major: work.statx_device_major,
            statx_device_minor: work.statx_device_minor,
            fs_type_magic: work.fs_type_magic,
            filesystem_id: work.filesystem_id,
        }
    };
    VerifiedCacheBudget {
        live_bytes: document.cache.live_bytes,
        live_inodes: document.cache.live_inodes,
        stage_bytes: document.cache.stage_bytes,
        stage_inodes: document.cache.stage_inodes,
        #[cfg(target_os = "linux")]
        runner_work,
    }
}

#[cfg(test)]
pub(super) fn test_budget() -> VerifiedCacheBudget {
    tests::parsed_budget(&tests::document())
}

#[cfg(test)]
#[path = "storage_admission_tests.rs"]
mod tests;

#[cfg(target_os = "linux")]
#[path = "storage_admission_mount.rs"]
mod mount;

#[cfg(any(target_os = "linux", test))]
#[path = "storage_admission_mountinfo.rs"]
mod mountinfo;

#[cfg(any(target_os = "linux", test))]
#[path = "storage_admission_identity.rs"]
mod identity;

#[path = "storage_admission_measure.rs"]
mod measure;
pub(super) use measure::{CacheTreeUsage, FreshCapacity, observe_cache_storage};

#[cfg(any(target_os = "linux", test))]
#[path = "storage_admission_validation.rs"]
mod validation;

#[path = "storage_admission_restore.rs"]
mod restore;
