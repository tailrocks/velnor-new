use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StorageCacheAdmissionV1 {
    pub(super) schema: String,
    pub(super) version: u32,
    pub(super) scope: ScopeV1,
    pub(super) reservation: ReservationV1,
    pub(super) runner: RunnerV1,
    pub(super) engine: EngineV1,
    pub(super) profile: ProfileV1,
    pub(super) sample: SampleV1,
    pub(super) instance_binding: InstanceBindingV1,
    pub(super) instance_binding_digest: String,
    pub(super) cache: CacheLimitsV1,
    pub(super) job_facts_mount: JobFactsMountV1,
    pub(super) projection_digest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ScopeV1 {
    pub(super) connection_id: String,
    pub(super) scale_set_id: i64,
    pub(super) binding_digest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ReservationV1 {
    pub(super) id: i64,
    pub(super) launch_intent_id: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RunnerV1 {
    pub(super) reference: RunnerReferenceV1,
    pub(super) container_id: String,
    pub(super) dind_container_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RunnerReferenceV1 {
    pub(super) id: i64,
    pub(super) name: String,
    pub(super) runner_scale_set_id: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct EngineV1 {
    pub(super) id: String,
    pub(super) docker_root_digest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ProfileV1 {
    pub(super) generation_id: String,
    pub(super) digest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SampleV1 {
    pub(super) generation_id: String,
    pub(super) observed_at_unix_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct InstanceBindingV1 {
    pub(super) schema_version: u32,
    pub(super) connection_id: String,
    pub(super) scale_set_id: i64,
    pub(super) reservation_id: i64,
    pub(super) intent_id: i64,
    pub(super) engine_id: String,
    pub(super) docker_root_digest: String,
    pub(super) sample_generation_id: String,
    pub(super) sample_observed_at_unix_ms: u64,
    pub(super) profile_generation_id: String,
    pub(super) profile_digest: String,
    pub(super) guest_boot_id: String,
    pub(super) runner_container_id: String,
    pub(super) dind_container_id: String,
    pub(super) roles: Vec<StorageRoleV1>,
    pub(super) cgroups: Vec<CgroupV1>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StorageRoleV1 {
    pub(super) role: String,
    pub(super) pool_id: String,
    pub(super) filesystem_instance_digest: String,
    pub(super) medium: String,
    pub(super) filesystem_kind: String,
    pub(super) mount_target: String,
    pub(super) mount_id: u64,
    pub(super) device_major: u32,
    pub(super) device_minor: u32,
    pub(super) statx_device_major: u32,
    pub(super) statx_device_minor: u32,
    pub(super) fs_type_magic: u64,
    pub(super) filesystem_id: u64,
    pub(super) total_bytes: u64,
    pub(super) free_bytes: u64,
    pub(super) available_bytes: u64,
    pub(super) inode_total: Option<u64>,
    pub(super) inode_free: Option<u64>,
    pub(super) inode_available: Option<u64>,
    pub(super) inode_policy: String,
    pub(super) charge_bytes: u64,
    pub(super) charge_inodes: u64,
    pub(super) retained_memory_bytes: u64,
    pub(super) owned_usage_bytes: u64,
    pub(super) owned_usage_inodes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CgroupV1 {
    pub(super) role: String,
    pub(super) container_id: String,
    pub(super) path: String,
    pub(super) memory_max_bytes: Option<u64>,
    pub(super) memory_current_bytes: u64,
    pub(super) cpu_quota_micros: Option<u64>,
    pub(super) cpu_period_micros: u64,
    pub(super) pids_max: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CacheLimitsV1 {
    pub(super) live_path: String,
    pub(super) stage_path: String,
    pub(super) live_bytes: u64,
    pub(super) live_inodes: u64,
    pub(super) stage_bytes: u64,
    pub(super) stage_inodes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct JobFactsMountV1 {
    pub(super) target: String,
    pub(super) read_only: bool,
    pub(super) host_directory_device: u64,
    pub(super) host_directory_inode: u64,
    pub(super) guest_mount_id: u64,
    pub(super) guest_device_major: u32,
    pub(super) guest_device_minor: u32,
    pub(super) guest_directory_device: u64,
    pub(super) guest_directory_inode: u64,
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ProjectionPreimageV1<'a> {
    pub(super) schema: &'a str,
    pub(super) version: u32,
    pub(super) scope: &'a ScopeV1,
    pub(super) reservation: &'a ReservationV1,
    pub(super) runner: &'a RunnerV1,
    pub(super) engine: &'a EngineV1,
    pub(super) profile: &'a ProfileV1,
    pub(super) sample: &'a SampleV1,
    pub(super) instance_binding: &'a InstanceBindingV1,
    pub(super) instance_binding_digest: &'a str,
    pub(super) cache: &'a CacheLimitsV1,
    pub(super) job_facts_mount: &'a JobFactsMountV1,
}
