use super::super::storage_admission_types::{
    InstanceBindingV1, JobFactsMountV1, ProjectionPreimageV1, StorageCacheAdmissionV1,
    StorageRoleV1,
};
use super::ROLE_NAMES;
use sha2::{Digest, Sha256};
use std::path::{Component, Path};

const INSTANCE_DOMAIN: &[u8] = b"velnor-instance-binding-v1\0";
const PROJECTION_DOMAIN: &[u8] = b"velnor-storage-cache-admission-projection-v1\0";

pub(super) fn validate_document(
    document: &StorageCacheAdmissionV1,
    runner_temp: &Path,
    runner_name: &str,
) -> Result<(), &'static str> {
    if document.schema != "velnor.storage-cache-admission" || document.version != 1 {
        return Err("cache_storage_projection_schema_invalid");
    }
    validate_scope(document)?;
    validate_instance_binding(document)?;
    validate_cache_paths(document, runner_temp)?;
    validate_mount_shape(&document.job_facts_mount)?;
    validate_roles(document)?;
    validate_cgroups(document)?;
    if document.runner.reference.name != runner_name {
        return Err("cache_storage_runner_binding_mismatch");
    }
    validate_digest(&document.instance_binding_digest)?;
    if canonical_instance_binding_digest(&document.instance_binding)?
        != document.instance_binding_digest
    {
        return Err("cache_storage_instance_binding_digest_mismatch");
    }
    validate_digest(&document.projection_digest)?;
    if canonical_projection_digest(document)? != document.projection_digest {
        return Err("cache_storage_projection_digest_mismatch");
    }
    Ok(())
}

fn validate_scope(document: &StorageCacheAdmissionV1) -> Result<(), &'static str> {
    let scope = &document.scope;
    let binding = &document.instance_binding;
    if !valid_text(&scope.connection_id, 256)
        || scope.scale_set_id <= 0
        || !is_digest(&scope.binding_digest)
        || document.reservation.id <= 0
        || document.reservation.launch_intent_id <= 0
        || document.runner.reference.id <= 0
        || document.runner.reference.runner_scale_set_id != scope.scale_set_id
        || !valid_text(&document.runner.reference.name, 255)
        || !is_digest(&document.engine.docker_root_digest)
        || !valid_text(&document.engine.id, 256)
        || !valid_text(&document.profile.generation_id, 256)
        || !is_digest(&document.profile.digest)
        || !valid_text(&document.sample.generation_id, 256)
        || document.sample.observed_at_unix_ms == 0
        || binding.connection_id != scope.connection_id
        || binding.scale_set_id != scope.scale_set_id
        || binding.reservation_id != document.reservation.id
        || binding.intent_id != document.reservation.launch_intent_id
        || binding.engine_id != document.engine.id
        || binding.docker_root_digest != document.engine.docker_root_digest
        || binding.profile_generation_id != document.profile.generation_id
        || binding.profile_digest != document.profile.digest
        || binding.sample_generation_id != document.sample.generation_id
        || binding.sample_observed_at_unix_ms != document.sample.observed_at_unix_ms
        || !valid_text(&document.runner.container_id, 128)
        || !valid_text(&document.runner.dind_container_id, 128)
    {
        return Err("cache_storage_projection_binding_invalid");
    }
    Ok(())
}

fn validate_instance_binding(document: &StorageCacheAdmissionV1) -> Result<(), &'static str> {
    let binding: &InstanceBindingV1 = &document.instance_binding;
    if binding.schema_version != 1
        || binding.guest_boot_id.is_empty()
        || binding.runner_container_id != document.runner.container_id
        || binding.dind_container_id != document.runner.dind_container_id
    {
        return Err("cache_storage_instance_binding_invalid");
    }
    Ok(())
}

fn validate_cache_paths(
    document: &StorageCacheAdmissionV1,
    runner_temp: &Path,
) -> Result<(), &'static str> {
    validate_clean_absolute(runner_temp)?;
    let live = runner_temp.join("velnor/cache");
    let stage = runner_temp.join("velnor/cache-staging");
    let live_text = live.to_str().ok_or("cache_storage_cache_path_invalid")?;
    let stage_text = stage.to_str().ok_or("cache_storage_cache_path_invalid")?;
    if document.cache.live_path != live_text
        || document.cache.stage_path != stage_text
        || document.cache.live_bytes == 0
        || document.cache.live_inodes == 0
        || document.cache.stage_bytes == 0
        || document.cache.stage_inodes == 0
    {
        return Err("cache_storage_cache_path_or_limit_invalid");
    }
    Ok(())
}

fn validate_mount_shape(mount: &JobFactsMountV1) -> Result<(), &'static str> {
    if mount.target != "/opt/velnor/job-facts"
        || !mount.read_only
        || mount.host_directory_inode == 0
        || mount.guest_mount_id == 0
        || mount.guest_directory_inode == 0
    {
        return Err("cache_storage_job_facts_mount_invalid");
    }
    Ok(())
}

fn validate_roles(document: &StorageCacheAdmissionV1) -> Result<(), &'static str> {
    let roles = &document.instance_binding.roles;
    if roles.len() != ROLE_NAMES.len()
        || roles
            .iter()
            .zip(ROLE_NAMES)
            .any(|(role, expected)| role.role != *expected || !valid_role(role))
    {
        return Err("cache_storage_roles_invalid");
    }
    let work = &roles[2];
    let live = &roles[10];
    let stage = &roles[11];
    if !same_filesystem(work, live)
        || !same_filesystem(work, stage)
        || document.cache.live_bytes != live.charge_bytes
        || document.cache.live_inodes != live.charge_inodes
        || document.cache.stage_bytes != stage.charge_bytes
        || document.cache.stage_inodes != stage.charge_inodes
        || document
            .cache
            .live_bytes
            .checked_add(document.cache.stage_bytes)
            .is_none_or(|sum| sum > work.charge_bytes)
        || document
            .cache
            .live_inodes
            .checked_add(document.cache.stage_inodes)
            .is_none_or(|sum| sum > work.charge_inodes)
    {
        return Err("cache_storage_alias_limits_invalid");
    }
    Ok(())
}

fn valid_role(role: &StorageRoleV1) -> bool {
    valid_text(&role.pool_id, 256)
        && is_digest(&role.filesystem_instance_digest)
        && matches!(role.medium.as_str(), "disk" | "tmpfs")
        && matches!(role.filesystem_kind.as_str(), "btrfs" | "tmpfs" | "other")
        && clean_absolute_text(&role.mount_target)
        && role.mount_id > 0
        && role.total_bytes > 0
        && role.available_bytes <= role.free_bytes
        && role.free_bytes <= role.total_bytes
        && matches!(role.inode_policy.as_str(), "finite" | "dynamic-btrfs")
        && role.charge_bytes > 0
        && role.charge_inodes > 0
        && inode_counts_valid(role)
        && role.owned_usage_bytes <= role.charge_bytes
        && role.owned_usage_inodes <= role.charge_inodes
}

fn inode_counts_valid(role: &StorageRoleV1) -> bool {
    match (role.inode_total, role.inode_free, role.inode_available) {
        (Some(total), Some(free), Some(available)) => available <= free && free <= total,
        (None, None, None) => role.inode_policy == "dynamic-btrfs",
        _ => false,
    }
}

fn same_filesystem(left: &StorageRoleV1, right: &StorageRoleV1) -> bool {
    left.pool_id == right.pool_id
        && left.filesystem_instance_digest == right.filesystem_instance_digest
        && left.medium == right.medium
        && left.filesystem_kind == right.filesystem_kind
        && left.device_major == right.device_major
        && left.device_minor == right.device_minor
        && left.statx_device_major == right.statx_device_major
        && left.statx_device_minor == right.statx_device_minor
        && left.fs_type_magic == right.fs_type_magic
        && left.filesystem_id == right.filesystem_id
}

fn validate_cgroups(document: &StorageCacheAdmissionV1) -> Result<(), &'static str> {
    let cgroups = &document.instance_binding.cgroups;
    if cgroups.len() != 2
        || cgroups[0].role != "dind"
        || cgroups[0].container_id != document.runner.dind_container_id
        || cgroups[1].role != "runner"
        || cgroups[1].container_id != document.runner.container_id
        || cgroups.iter().any(|row| {
            !clean_absolute_text(&row.path)
                || row
                    .memory_max_bytes
                    .is_some_and(|max| max == 0 || row.memory_current_bytes > max)
                || row.cpu_quota_micros == Some(0)
                || row.cpu_period_micros == 0
                || row.pids_max == Some(0)
        })
    {
        return Err("cache_storage_cgroups_invalid");
    }
    Ok(())
}

fn validate_digest(value: &str) -> Result<(), &'static str> {
    if is_digest(value) {
        Ok(())
    } else {
        Err("cache_storage_digest_invalid")
    }
}

fn is_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn valid_text(value: &str, max_bytes: usize) -> bool {
    !value.is_empty()
        && value.len() <= max_bytes
        && !value.bytes().any(|byte| byte.is_ascii_control())
}

fn validate_clean_absolute(path: &Path) -> Result<(), &'static str> {
    if !path.is_absolute()
        || path.as_os_str().len() > 4_096
        || path.components().any(|component| {
            matches!(
                component,
                Component::CurDir | Component::ParentDir | Component::Prefix(_)
            )
        })
    {
        return Err("cache_storage_cache_path_invalid");
    }
    Ok(())
}

fn clean_absolute_text(value: &str) -> bool {
    let path = Path::new(value);
    path.is_absolute()
        && path.as_os_str().len() <= 4_096
        && !path.components().any(|component| {
            matches!(
                component,
                Component::CurDir | Component::ParentDir | Component::Prefix(_)
            )
        })
}

fn digest(domain: &[u8], bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(domain);
    hasher.update(bytes);
    lower_hex(&hasher.finalize())
}

fn lower_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

pub(super) fn canonical_projection_digest(
    document: &StorageCacheAdmissionV1,
) -> Result<String, &'static str> {
    let preimage = ProjectionPreimageV1 {
        schema: &document.schema,
        version: document.version,
        scope: &document.scope,
        reservation: &document.reservation,
        runner: &document.runner,
        engine: &document.engine,
        profile: &document.profile,
        sample: &document.sample,
        instance_binding: &document.instance_binding,
        instance_binding_digest: &document.instance_binding_digest,
        cache: &document.cache,
        job_facts_mount: &document.job_facts_mount,
    };
    let bytes = serde_json::to_vec(&preimage).map_err(|_| "cache_storage_projection_invalid")?;
    Ok(digest(PROJECTION_DOMAIN, &bytes))
}

pub(super) fn canonical_instance_binding_digest(
    binding: &InstanceBindingV1,
) -> Result<String, &'static str> {
    let bytes = serde_json::to_vec(binding).map_err(|_| "cache_storage_projection_invalid")?;
    Ok(digest(INSTANCE_DOMAIN, &bytes))
}
