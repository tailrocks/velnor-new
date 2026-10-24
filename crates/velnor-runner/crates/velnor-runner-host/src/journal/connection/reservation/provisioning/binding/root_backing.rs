use std::collections::BTreeMap;

use sha2::{Digest, Sha256};

use crate::error::HostError;
use crate::journal::connection::{StorageInodePolicy, StorageMedium, StorageObserver, StorageRole};
use crate::launch::bootstrap::VerifiedStorageResourceBinding;

use super::super::admission::sql_i64;

pub(super) async fn validate_roles(
    profile: &super::super::super::storage_budget::StorageProfile,
    connection: &turso::Connection,
    connection_id: &str,
    reservation_id: i64,
    binding: &VerifiedStorageResourceBinding,
) -> Result<(), HostError> {
    if binding.roles().len() != StorageRole::PHYSICAL.len() {
        return Err(HostError::Journal);
    }
    let sample_pools = load_sample_pools(profile, connection, reservation_id).await?;
    let mut seen = BTreeMap::new();
    for observed in binding.roles() {
        validate_observed_role(profile, observed, &sample_pools, &mut seen)?;
    }
    if seen.len() != StorageRole::PHYSICAL.len() {
        return Err(HostError::Journal);
    }
    validate_disk_identity(&seen)?;
    validate_root_backings(connection_id, reservation_id, binding, &seen)?;
    super::attachments::validate_attachments(binding, &seen)?;
    Ok(())
}

type SamplePools = BTreeMap<String, (String, String, Option<String>)>;

async fn load_sample_pools(
    profile: &super::super::super::storage_budget::StorageProfile,
    connection: &turso::Connection,
    reservation_id: i64,
) -> Result<SamplePools, HostError> {
    let mut sample_pools = BTreeMap::new();
    let mut rows = connection
        .query(
            "SELECT pool_id, scope, medium, filesystem_kind, inode_policy, filesystem_instance_digest, max_bytes, max_inodes FROM capacity_reservation_provisioning_pools WHERE reservation_id = ?1",
            [reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        let pool_id: String = row.get(0).map_err(|_| HostError::Journal)?;
        let scope: String = row.get(1).map_err(|_| HostError::Journal)?;
        let medium: String = row.get(2).map_err(|_| HostError::Journal)?;
        let kind: String = row.get(3).map_err(|_| HostError::Journal)?;
        let inode_policy: String = row.get(4).map_err(|_| HostError::Journal)?;
        let digest: Option<String> = row.get(5).map_err(|_| HostError::Journal)?;
        let max_bytes: i64 = row.get(6).map_err(|_| HostError::Journal)?;
        let max_inodes: i64 = row.get(7).map_err(|_| HostError::Journal)?;
        let expected = profile.pool_limit(&pool_id)?;
        if medium != expected.medium().as_str()
            || inode_policy != expected.inode_policy().as_str()
            || sql_i64(expected.max_bytes())? != max_bytes
            || sql_i64(expected.max_inodes())? != max_inodes
        {
            return Err(HostError::Journal);
        }
        sample_pools.insert(pool_id, (scope, kind, digest));
    }
    if sample_pools.len() != 9 {
        return Err(HostError::Journal);
    }
    Ok(sample_pools)
}

fn validate_observed_role<'a>(
    profile: &super::super::super::storage_budget::StorageProfile,
    observed: &'a crate::launch::bootstrap::ObservedStorageRole,
    sample_pools: &SamplePools,
    seen: &mut BTreeMap<StorageRole, &'a str>,
) -> Result<(), HostError> {
    let role = observed.role();
    let limit = profile.role_limit(role)?;
    let pool_limit = profile.pool_limit(limit.pool_id())?;
    let (charge_bytes, charge_inodes, retained_memory) = observed.charges();
    let (total, free, available) = observed.byte_counts();
    let (inode_total, inode_free, inode_available) = observed.inode_counts();
    let (owned_bytes, owned_inodes) = observed.owned_usage();
    let (scope, expected_kind, sample_digest) = sample_pools
        .get(limit.pool_id())
        .ok_or(HostError::Journal)?;
    if observed.pool_id() != limit.pool_id()
        || observed.medium() != limit.medium()
        || observed.filesystem_kind().as_str() != expected_kind
        || observed.inode_policy() != pool_limit.inode_policy()
        || charge_bytes != limit.max_bytes()
        || charge_inodes != limit.max_inodes()
        || retained_memory != limit.retained_memory_bytes()
        || total == 0
        || free > total
        || available > free
        || (observed.medium() == StorageMedium::Tmpfs
            && (*scope != "exclusive-tmpfs"
                || observed.filesystem_kind().as_str() != "tmpfs"
                || total != charge_bytes
                || inode_total != Some(charge_inodes)
                || available
                    .checked_add(owned_bytes)
                    .is_none_or(|value| value < charge_bytes)
                || inode_available
                    .and_then(|value| value.checked_add(owned_inodes))
                    .is_none_or(|value| value < charge_inodes)))
        || (observed.medium() == StorageMedium::Disk
            && (*scope != "shared-disk"
                || sample_digest.as_deref() != Some(observed.filesystem_instance_digest())
                || owned_bytes != 0
                || owned_inodes != 0))
    {
        return Err(HostError::Journal);
    }
    validate_observed_inode_values(
        observed.inode_policy(),
        observed.filesystem_kind().as_str(),
        inode_total,
        inode_free,
        inode_available,
    )?;
    if seen
        .insert(role, observed.filesystem_instance_digest())
        .is_some()
    {
        return Err(HostError::Journal);
    }
    Ok(())
}

fn validate_disk_identity(seen: &BTreeMap<StorageRole, &str>) -> Result<(), HostError> {
    let disk_ids = [
        StorageRole::RunnerRoot,
        StorageRole::DindRoot,
        StorageRole::EngineMetadata,
    ]
    .map(|role| seen.get(&role).copied().ok_or(HostError::Journal))
    .into_iter()
    .collect::<Result<Vec<_>, _>>()?;
    if disk_ids.windows(2).any(|pair| pair[0] != pair[1]) {
        return Err(HostError::Journal);
    }
    Ok(())
}

fn validate_root_backings(
    connection_id: &str,
    reservation_id: i64,
    binding: &VerifiedStorageResourceBinding,
    roles: &BTreeMap<StorageRole, &str>,
) -> Result<(), HostError> {
    let mappings = binding.root_backings();
    if mappings.len() != 2 {
        return Err(HostError::Journal);
    }
    let mut seen = BTreeMap::new();
    for mapping in mappings {
        let role = mapping.role();
        let (role_text, observer, container_id) = match role {
            StorageRole::RunnerRoot => (
                "runner-root",
                StorageObserver::Runner,
                binding.runner_container_id(),
            ),
            StorageRole::DindRoot => (
                "dind-root",
                StorageObserver::Dind,
                binding.dind_container_id(),
            ),
            _ => return Err(HostError::Identity),
        };
        let logical = binding
            .attachments()
            .iter()
            .find(|attachment| {
                attachment.role() == role
                    && attachment.observer() == observer
                    && attachment.target() == "/"
            })
            .ok_or(HostError::Journal)?;
        let capacity_digest = roles.get(&role).ok_or(HostError::Journal)?;
        if mapping.reservation_id() != reservation_id
            || mapping.operation_id() != binding.operation_id()
            || mapping.connection_id() != connection_id
            || mapping.engine_id() != binding.engine_id()
            || mapping.docker_root_digest() != binding.docker_root_digest()
            || mapping.sample_generation_id() != binding.sample_generation_id()
            || mapping.container_id() != container_id
            || !valid_runtime_image_id(mapping.image_runtime_id())
            || !valid_git_sha(mapping.image_source_sha())
            || mapping.logical_mount_id() != logical.mount_id()
            || mapping.logical_device() != logical.device()
            || mapping.logical_statx_device() != logical.statx_device()
            || mapping.logical_fs_type_magic() != logical.fs_type_magic()
            || mapping.logical_filesystem_id() != logical.filesystem_id()
            || mapping.logical_filesystem_digest() != logical.filesystem_instance_digest()
            || mapping.backing_filesystem_digest() != *capacity_digest
            || !super::super::super::storage_budget::valid_digest(
                mapping.logical_filesystem_digest(),
            )
            || !super::super::super::storage_budget::valid_digest(
                mapping.backing_filesystem_digest(),
            )
            || !super::super::super::storage_budget::valid_digest(mapping.mapping_digest())
            || sha256(mapping.canonical_record().as_bytes()) != mapping.mapping_digest()
            || seen.insert(role_text, mapping).is_some()
        {
            return Err(HostError::Identity);
        }
    }
    if seen.len() != 2 {
        return Err(HostError::Journal);
    }
    Ok(())
}

fn sha256(bytes: &[u8]) -> String {
    use std::fmt::Write as _;

    let mut digest = Sha256::new();
    digest.update(b"velnor-root-backing-map-v1\0");
    digest.update(bytes);
    let mut output = String::with_capacity(64);
    for byte in digest.finalize() {
        let _written = write!(output, "{byte:02x}");
    }
    output
}

fn valid_runtime_image_id(value: &str) -> bool {
    (1..=255).contains(&value.len()) && !value.chars().any(char::is_control)
}

fn valid_git_sha(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub(super) fn root_backing_role(role: StorageRole) -> Result<&'static str, HostError> {
    match role {
        StorageRole::RunnerRoot => Ok("runner-root"),
        StorageRole::DindRoot => Ok("dind-root"),
        _ => Err(HostError::Identity),
    }
}

fn validate_observed_inode_values(
    policy: StorageInodePolicy,
    filesystem_kind: &str,
    total: Option<u64>,
    free: Option<u64>,
    available: Option<u64>,
) -> Result<(), HostError> {
    match (policy, total, free, available) {
        (StorageInodePolicy::Finite, Some(total), Some(free), Some(available))
            if total > 0 && free <= total && available <= free =>
        {
            Ok(())
        }
        (StorageInodePolicy::DynamicBtrfs, Some(0), Some(0), Some(0))
            if filesystem_kind == "btrfs" =>
        {
            Ok(())
        }
        _ => Err(HostError::Journal),
    }
}
