use super::{
    Cache, Cgroup, Document, Engine, InstanceBinding, JobFactsMount, Profile, Reservation, Runner,
    RunnerReference, Sample, Scope, StorageRole,
};

pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) struct DocumentParts
{
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) scope:
        Scope,
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) reservation:
        Reservation,
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) runner:
        Runner,
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) engine:
        Engine,
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) profile:
        Profile,
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) sample:
        Sample,
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) instance_binding:
        InstanceBinding,
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) cache:
        Cache,
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) job_facts_mount:
        JobFactsMount,
}

pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) fn new_document(
    parts: DocumentParts,
) -> Document {
    Document {
        schema: "velnor.storage-cache-admission".to_owned(),
        version: 1,
        scope: parts.scope,
        reservation: parts.reservation,
        runner: parts.runner,
        engine: parts.engine,
        profile: parts.profile,
        sample: parts.sample,
        instance_binding: parts.instance_binding,
        instance_binding_digest: String::new(),
        cache: parts.cache,
        job_facts_mount: parts.job_facts_mount,
        projection_digest: String::new(),
    }
}

pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) fn scope(
    connection_id: String,
    scale_set_id: i64,
    binding_digest: String,
) -> Scope {
    Scope {
        connection_id,
        scale_set_id,
        binding_digest,
    }
}
pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) fn reservation(
    id: i64,
    launch_intent_id: i64,
) -> Reservation {
    Reservation {
        id,
        launch_intent_id,
    }
}
pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) fn runner(
    id: i64,
    name: String,
    scale_set_id: i64,
    container_id: String,
    dind_container_id: String,
) -> Runner {
    Runner {
        reference: RunnerReference {
            id,
            name,
            runner_scale_set_id: scale_set_id,
        },
        container_id,
        dind_container_id,
    }
}
pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) fn engine(
    id: String,
    docker_root_digest: String,
) -> Engine {
    Engine {
        id,
        docker_root_digest,
    }
}
pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) fn profile(
    generation_id: String,
    digest: String,
) -> Profile {
    Profile {
        generation_id,
        digest,
    }
}
pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) fn sample(
    generation_id: String,
    observed_at_unix_ms: u64,
) -> Sample {
    Sample {
        generation_id,
        observed_at_unix_ms,
    }
}
pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) struct InstanceBindingParts
{
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) connection_id:
        String,
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) scale_set_id:
        i64,
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) reservation_id:
        i64,
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) intent_id:
        i64,
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) engine_id:
        String,
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) docker_root_digest:
        String,
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) sample_generation_id:
        String,
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) sample_observed_at_unix_ms:
        u64,
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) profile_generation_id:
        String,
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) profile_digest:
        String,
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) guest_boot_id:
        String,
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) runner_container_id:
        String,
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) dind_container_id:
        String,
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) roles:
        Vec<StorageRole>,
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) cgroups:
        Vec<Cgroup>,
}

pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) fn instance_binding(
    parts: InstanceBindingParts,
) -> InstanceBinding {
    InstanceBinding {
        schema_version: 1,
        connection_id: parts.connection_id,
        scale_set_id: parts.scale_set_id,
        reservation_id: parts.reservation_id,
        intent_id: parts.intent_id,
        engine_id: parts.engine_id,
        docker_root_digest: parts.docker_root_digest,
        sample_generation_id: parts.sample_generation_id,
        sample_observed_at_unix_ms: parts.sample_observed_at_unix_ms,
        profile_generation_id: parts.profile_generation_id,
        profile_digest: parts.profile_digest,
        guest_boot_id: parts.guest_boot_id,
        runner_container_id: parts.runner_container_id,
        dind_container_id: parts.dind_container_id,
        roles: parts.roles,
        cgroups: parts.cgroups,
    }
}
pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) fn cache(
    live_path: String,
    stage_path: String,
    live_bytes: u64,
    live_inodes: u64,
    stage_bytes: u64,
    stage_inodes: u64,
) -> Cache {
    Cache {
        live_path,
        stage_path,
        live_bytes,
        live_inodes,
        stage_bytes,
        stage_inodes,
    }
}
pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) fn job_facts_mount(
    target: String,
    host_directory_device: u64,
    host_directory_inode: u64,
    guest_mount_id: u64,
    guest_device: (u32, u32),
    guest_directory_device: u64,
    guest_directory_inode: u64,
) -> JobFactsMount {
    JobFactsMount {
        target,
        read_only: true,
        host_directory_device,
        host_directory_inode,
        guest_mount_id,
        guest_device_major: guest_device.0,
        guest_device_minor: guest_device.1,
        guest_directory_device,
        guest_directory_inode,
    }
}

/// Encode a Linux `dev_t` from its measured major and minor values.
pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) const fn linux_device_number(
    major: u32,
    minor: u32,
) -> u64 {
    (((major & 0x0fff) as u64) << 8)
        | (((major & 0xffff_f000) as u64) << 32)
        | ((minor & 0x00ff) as u64)
        | (((minor & 0xffff_ff00) as u64) << 12)
}

pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) struct StorageRoleParts
{
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) role:
        String,
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) pool_id:
        String,
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) filesystem_instance_digest:
        String,
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) medium:
        String,
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) filesystem_kind:
        String,
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) mount_target:
        String,
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) mount_id:
        u64,
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) device:
        (u32, u32),
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) statx_device:
        (u32, u32),
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) fs_type_magic:
        u64,
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) filesystem_id:
        u64,
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) total_bytes:
        u64,
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) free_bytes:
        u64,
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) available_bytes:
        u64,
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) inode_total:
        Option<u64>,
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) inode_free:
        Option<u64>,
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) inode_available:
        Option<u64>,
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) inode_policy:
        String,
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) charge_bytes:
        u64,
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) charge_inodes:
        u64,
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) retained_memory_bytes:
        u64,
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) owned_usage_bytes:
        u64,
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) owned_usage_inodes:
        u64,
}

pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) fn storage_role(
    parts: StorageRoleParts,
) -> StorageRole {
    StorageRole {
        role: parts.role,
        pool_id: parts.pool_id,
        filesystem_instance_digest: parts.filesystem_instance_digest,
        medium: parts.medium,
        filesystem_kind: parts.filesystem_kind,
        mount_target: parts.mount_target,
        mount_id: parts.mount_id,
        device_major: parts.device.0,
        device_minor: parts.device.1,
        statx_device_major: parts.statx_device.0,
        statx_device_minor: parts.statx_device.1,
        fs_type_magic: parts.fs_type_magic,
        filesystem_id: parts.filesystem_id,
        total_bytes: parts.total_bytes,
        free_bytes: parts.free_bytes,
        available_bytes: parts.available_bytes,
        inode_total: parts.inode_total,
        inode_free: parts.inode_free,
        inode_available: parts.inode_available,
        inode_policy: parts.inode_policy,
        charge_bytes: parts.charge_bytes,
        charge_inodes: parts.charge_inodes,
        retained_memory_bytes: parts.retained_memory_bytes,
        owned_usage_bytes: parts.owned_usage_bytes,
        owned_usage_inodes: parts.owned_usage_inodes,
    }
}

pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) struct CgroupParts
{
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) role:
        String,
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) container_id:
        String,
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) path:
        String,
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) memory_max_bytes:
        u64,
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) memory_current_bytes:
        u64,
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) cpu_quota_micros:
        u64,
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) cpu_period_micros:
        u64,
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) pids_max:
        u64,
}

pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) fn cgroup(
    parts: CgroupParts,
) -> Cgroup {
    Cgroup {
        role: parts.role,
        container_id: parts.container_id,
        path: parts.path,
        memory_max_bytes: Some(parts.memory_max_bytes),
        memory_current_bytes: parts.memory_current_bytes,
        cpu_quota_micros: Some(parts.cpu_quota_micros),
        cpu_period_micros: parts.cpu_period_micros,
        pids_max: Some(parts.pids_max),
    }
}
