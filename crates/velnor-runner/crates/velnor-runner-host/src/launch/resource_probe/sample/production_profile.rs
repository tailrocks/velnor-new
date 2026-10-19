//! Fixed storage policy from checked startup data.

use sha2::{Digest, Sha256};
use std::time::Instant;

use crate::error::HostError;
use crate::journal::{
    StorageInodePolicy, StorageMedium, StoragePoolLimit, StorageProfile, StorageRole,
    StorageRoleLimit,
};
use crate::launch::VerifiedWorkerImages;
use crate::worker::ResourceBudget;

use super::super::provider::ProbeImagePreparationEvidence;
use super::{Observation, ProbeStartupEvidence};

const POLICY_DOMAIN: &[u8] = b"velnor-storage-policy-one-job-v1\0";
const PROFILE_DOMAIN: &[u8] = b"velnor-storage-startup-profile-v1\0";
const TOPOLOGY_REVISION: &str = "readonly-runner-shared-tmpfs-v1";
const DISK_POOL_BYTES: u64 = 128 * GIB;
const DISK_MINIMUM_RESERVE_BYTES: u64 = 64 * GIB;
const DISK_IMPORT_BASELINE_BYTES: u64 = 32 * GIB;
const DISK_ROOT_ROLE_BYTES: u64 = 128 * MIB;
const ENGINE_METADATA_BYTES: u64 = 8 * GIB;
const CACHE_ALIAS_BYTES: u64 = GIB;
const CACHE_LIVE_INODES: u64 = 131_072;
const CACHE_STAGE_INODES: u64 = 8_192;

const GIB: u64 = 1 << 30;
const MIB: u64 = 1 << 20;

const TMPFS_ROLES: [(StorageRole, u64, u64); 8] = [
    (StorageRole::RunnerHome, 2 * GIB, 262_144),
    (StorageRole::RunnerWork, 4 * GIB, 524_288),
    (StorageRole::RunnerTmp, 512 * MIB, 32_768),
    (StorageRole::RunnerShm, 64 * MIB, 4_096),
    (StorageRole::SharedRun, 16 * MIB, 4_096),
    (StorageRole::DindData, 4 * GIB, 262_144),
    (StorageRole::DindTmp, 512 * MIB, 32_768),
    (StorageRole::DindShm, 64 * MIB, 4_096),
];

pub(in crate::launch::resource_probe) struct StartupStorageProfile {
    profile: StorageProfile,
}

impl StartupStorageProfile {
    pub(in crate::launch::resource_probe) fn into_profile(self) -> StorageProfile {
        self.profile
    }
}

/// Build the first policy after all required image imports and cleanup checks.
pub(in crate::launch::resource_probe) fn build(
    observation: &Observation,
    worker_images: &VerifiedWorkerImages,
    budget: ResourceBudget,
    max_jobs: u32,
) -> Option<StartupStorageProfile> {
    if max_jobs == 0
        || !observation.is_recent_at(Instant::now())
        || observation.engine_id() != worker_images.engine_id()
        || observation.docker_root_digest() != worker_images.docker_root_digest()
    {
        return None;
    }
    let startup_images = StartupImagePreparationReceipt::new(observation, worker_images)?;
    let disk_pool_id = super::storage_admission::disk_pool_identity(observation)?;
    let candidate = make_profile(&"0".repeat(32), &disk_pool_id).ok()?;
    let generation_id = profile_generation(
        candidate.profile_digest(),
        observation,
        worker_images,
        startup_images.probe_image,
        &disk_pool_id,
    );
    let profile = make_profile(&generation_id, &disk_pool_id).ok()?;
    if profile.profile_digest() != candidate.profile_digest()
        || profile.validate_provisioning_contract().is_err()
    {
        return None;
    }

    // Import peak is zero only for this complete, already-imported image set.
    // Both production import adapters reject missing images once this profile
    // is persisted. The 64 GiB floor remains even when the peak is zero.
    let remaining_import_peak = startup_images.remaining_import_peak_bytes;
    let required_reserve = DISK_MINIMUM_RESERVE_BYTES
        .max(DISK_IMPORT_BASELINE_BYTES.checked_add(remaining_import_peak)?);
    if profile
        .pools()
        .iter()
        .find(|pool| pool.medium() == StorageMedium::Disk)
        .is_none_or(|pool| pool.reserve_bytes() != required_reserve)
    {
        return None;
    }

    let admission = observation.storage_provisioning_admission(
        &profile,
        worker_images.engine_id(),
        worker_images.docker_root_digest(),
    )?;
    let pair_memory = budget.pair().memory_bytes;
    let retained_memory = retained_tmpfs_bytes()?;
    let all_jobs_memory = pair_memory
        .checked_add(retained_memory)?
        .checked_mul(u64::from(max_jobs))?
        .checked_add(budget.admission_reserve().memory_bytes)?;
    if all_jobs_memory > observation.memory_total_bytes {
        return None;
    }
    admission
        .validate_for(
            &profile,
            observation.sample_generation_id(),
            worker_images.engine_id(),
            worker_images.docker_root_digest(),
            pair_memory,
            Instant::now(),
        )
        .ok()?;
    Some(StartupStorageProfile { profile })
}

pub(in crate::launch) fn retained_tmpfs_bytes() -> Option<u64> {
    TMPFS_ROLES
        .iter()
        .try_fold(0_u64, |total, (_, bytes, _)| total.checked_add(*bytes))
}

struct StartupImagePreparationReceipt<'a> {
    probe_image: &'a ProbeImagePreparationEvidence,
    remaining_import_peak_bytes: u64,
}

impl<'a> StartupImagePreparationReceipt<'a> {
    fn new(observation: &'a Observation, worker_images: &VerifiedWorkerImages) -> Option<Self> {
        let evidence: &'a ProbeStartupEvidence = observation.startup_evidence();
        let cleanup = evidence.cleanup();
        let probe_image = evidence.image();
        if cleanup.operation_id() != observation.sample_generation_id()
            || cleanup.engine_id() != worker_images.engine_id()
            || cleanup.docker_root_digest() != worker_images.docker_root_digest()
            || cleanup.container_id().is_empty()
            || cleanup.operation_name()
                != format!(
                    "velnor-resource-probe-{}",
                    observation.sample_generation_id()
                )
            || probe_image.platform() != worker_images.platform()
            || probe_image.daemon_architecture() != worker_images.daemon_architecture()
            || probe_image.source_revision() != worker_images.source_sha()
            || !source_revision(worker_images.source_sha())
            || !digest_reference(probe_image.runtime_id())
            || !digest(probe_image.binding_fingerprint())
            || !digest(probe_image.workspace_cleanup_digest())
            || !digest(worker_images.release_workspace_cleanup_digest())
            || !complete_worker_image(worker_images.runner())
            || !complete_worker_image(worker_images.dind())
        {
            return None;
        }
        Some(Self {
            probe_image,
            remaining_import_peak_bytes: 0,
        })
    }
}

fn complete_worker_image(image: &crate::launch::VerifiedImageIdentity) -> bool {
    digest_reference(image.runtime_id())
        && digest(image.archive_sha256())
        && digest(image.archive_index_digest())
        && image.image_index_digest().is_some_and(digest_reference)
        && image.image_index_size().is_some_and(|size| size > 0)
        && digest_reference(image.manifest_digest())
        && image.manifest_size() > 0
        && digest_reference(image.config_digest())
}

fn make_profile(generation_id: &str, disk_pool_id: &str) -> Result<StorageProfile, HostError> {
    let disk_pool = disk_pool_id.to_owned();
    let disk_reserve = DISK_MINIMUM_RESERVE_BYTES.max(DISK_IMPORT_BASELINE_BYTES);
    let mut pools = vec![StoragePoolLimit::new_with_inode_policy(
        disk_pool.clone(),
        StorageMedium::Disk,
        StorageInodePolicy::DynamicBtrfs,
        DISK_POOL_BYTES,
        0,
        disk_reserve,
        0,
    )?];
    let mut roles = vec![
        StorageRoleLimit::new(
            StorageRole::RunnerRoot,
            disk_pool.clone(),
            StorageMedium::Disk,
            DISK_ROOT_ROLE_BYTES,
            4_096,
            0,
        )?,
        StorageRoleLimit::new(
            StorageRole::DindRoot,
            disk_pool.clone(),
            StorageMedium::Disk,
            DISK_ROOT_ROLE_BYTES,
            4_096,
            0,
        )?,
        StorageRoleLimit::new(
            StorageRole::EngineMetadata,
            disk_pool,
            StorageMedium::Disk,
            ENGINE_METADATA_BYTES,
            65_536,
            0,
        )?,
    ];

    for (role, bytes, inodes) in TMPFS_ROLES {
        let pool_id = policy_pool_id(role);
        pools.push(StoragePoolLimit::new_with_inode_policy(
            pool_id.clone(),
            StorageMedium::Tmpfs,
            StorageInodePolicy::Finite,
            bytes,
            inodes,
            0,
            0,
        )?);
        roles.push(StorageRoleLimit::new(
            role,
            pool_id,
            StorageMedium::Tmpfs,
            bytes,
            inodes,
            bytes,
        )?);
    }
    let work_pool = policy_pool_id(StorageRole::RunnerWork);
    roles.extend([
        StorageRoleLimit::new_alias(
            StorageRole::CacheLive,
            work_pool.clone(),
            StorageMedium::Tmpfs,
            StorageRole::RunnerWork,
            CACHE_ALIAS_BYTES,
            CACHE_LIVE_INODES,
        )?,
        StorageRoleLimit::new_alias(
            StorageRole::CacheStage,
            work_pool,
            StorageMedium::Tmpfs,
            StorageRole::RunnerWork,
            CACHE_ALIAS_BYTES,
            CACHE_STAGE_INODES,
        )?,
    ]);
    StorageProfile::new(generation_id.to_owned(), pools, roles)
}

fn policy_pool_id(role: StorageRole) -> String {
    hash_text(&[
        b"velnor-storage-logical-pool-v1\0",
        role.as_str().as_bytes(),
    ])
}

fn profile_generation(
    storage_profile_digest: &str,
    observation: &Observation,
    worker_images: &VerifiedWorkerImages,
    probe_image: &ProbeImagePreparationEvidence,
    disk_pool_id: &str,
) -> String {
    let mut digest = Sha256::new();
    digest.update(PROFILE_DOMAIN);
    update(&mut digest, POLICY_DOMAIN);
    update(&mut digest, TOPOLOGY_REVISION.as_bytes());
    update(&mut digest, storage_profile_digest.as_bytes());
    update(&mut digest, worker_images.engine_id().as_bytes());
    update(&mut digest, worker_images.docker_root_digest().as_bytes());
    update(&mut digest, worker_images.daemon_architecture().as_bytes());
    update(&mut digest, worker_images.platform().as_bytes());
    update(&mut digest, worker_images.source_sha().as_bytes());
    update(&mut digest, disk_pool_id.as_bytes());
    update(&mut digest, observation.record.guest_boot_id.as_bytes());
    update(
        &mut digest,
        b"verified-worker-and-probe-workspace-removal-v1",
    );
    update_worker_image(&mut digest, worker_images.runner());
    update_worker_image(&mut digest, worker_images.dind());
    update(&mut digest, probe_image.runtime_id().as_bytes());
    update(&mut digest, probe_image.source_revision().as_bytes());
    update(&mut digest, probe_image.binding_fingerprint().as_bytes());
    digest.update(observation.record.docker_root_filesystem_id.to_be_bytes());
    digest.update(observation.record.docker_root_fs_type_magic.to_be_bytes());
    digest.update(observation.record.docker_root_device_major.to_be_bytes());
    digest.update(observation.record.docker_root_device_minor.to_be_bytes());
    digest.update(
        observation
            .record
            .docker_root_statx_device_major
            .to_be_bytes(),
    );
    digest.update(
        observation
            .record
            .docker_root_statx_device_minor
            .to_be_bytes(),
    );
    hex_prefix(&digest.finalize())
}

fn update_worker_image(digest: &mut Sha256, image: &crate::launch::VerifiedImageIdentity) {
    update(digest, image.runtime_id().as_bytes());
    update(digest, image.archive_sha256().as_bytes());
    update(digest, image.archive_index_digest().as_bytes());
    if let Some(index) = image.image_index_digest() {
        update(digest, index.as_bytes());
    }
    digest.update(image.image_index_size().unwrap_or_default().to_be_bytes());
    update(digest, image.manifest_digest().as_bytes());
    digest.update(image.manifest_size().to_be_bytes());
    update(digest, image.config_digest().as_bytes());
}

fn update(digest: &mut Sha256, field: &[u8]) {
    let length = u64::try_from(field.len()).unwrap_or(u64::MAX);
    digest.update(length.to_be_bytes());
    digest.update(field);
}

fn hash_text(fields: &[&[u8]]) -> String {
    let mut digest = Sha256::new();
    for field in fields {
        update(&mut digest, field);
    }
    hex(&digest.finalize())
}

fn hex_prefix(bytes: &[u8]) -> String {
    let mut result = String::with_capacity(32);
    for byte in bytes.iter().take(16) {
        result.push(char::from(b"0123456789abcdef"[usize::from(byte >> 4)]));
        result.push(char::from(b"0123456789abcdef"[usize::from(byte & 0x0f)]));
    }
    result
}

fn hex(bytes: &[u8]) -> String {
    let mut result = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        result.push(char::from(b"0123456789abcdef"[usize::from(byte >> 4)]));
        result.push(char::from(b"0123456789abcdef"[usize::from(byte & 0x0f)]));
    }
    result
}

fn digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn digest_reference(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(digest)
}

fn source_revision(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
