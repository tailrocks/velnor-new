use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) struct Document
{
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) schema:
        String,
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) version:
        u32,
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
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) instance_binding_digest:
        String,
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) guest_image_sha256:
        String,
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) cache:
        Cache,
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) job_facts_mount:
        JobFactsMount,
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) projection_digest:
        String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) struct Scope {
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) connection_id:
        String,
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) scale_set_id:
        i64,
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) binding_digest:
        String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) struct Reservation
{
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) id: i64,
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) launch_intent_id:
        i64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) struct Runner
{
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) reference:
        RunnerReference,
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) container_id:
        String,
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) dind_container_id:
        String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) struct RunnerReference
{
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) id: i64,
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) name:
        String,
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) runner_scale_set_id:
        i64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) struct Engine
{
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) id:
        String,
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) docker_root_digest:
        String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) struct Profile
{
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) generation_id:
        String,
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) digest:
        String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) struct Sample
{
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) generation_id:
        String,
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) observed_at_unix_ms:
        u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) struct InstanceBinding
{
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) schema_version:
        u32,
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
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) struct StorageRole
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
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) device_major:
        u32,
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) device_minor:
        u32,
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) statx_device_major:
        u32,
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) statx_device_minor:
        u32,
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

impl StorageRole {
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) const fn device(
        &self,
    ) -> (u32, u32) {
        (self.device_major, self.device_minor)
    }

    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) const fn statx_device(
        &self,
    ) -> (u32, u32) {
        (self.statx_device_major, self.statx_device_minor)
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) struct Cgroup
{
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) role:
        String,
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) container_id:
        String,
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) path:
        String,
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) memory_max_bytes:
        Option<u64>,
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) memory_current_bytes:
        u64,
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) cpu_quota_micros:
        Option<u64>,
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) cpu_period_micros:
        u64,
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) pids_max:
        Option<u64>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) struct Cache {
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) live_path:
        String,
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) stage_path:
        String,
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) live_bytes:
        u64,
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) live_inodes:
        u64,
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) stage_bytes:
        u64,
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) stage_inodes:
        u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) struct JobFactsMount
{
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) target:
        String,
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) read_only:
        bool,
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) host_directory_device:
        u64,
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) host_directory_inode:
        u64,
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) guest_mount_id:
        u64,
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) guest_device_major:
        u32,
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) guest_device_minor:
        u32,
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) guest_directory_device:
        u64,
    pub(in crate::journal::connection::reservation::storage_budget::job_facts_projection) guest_directory_inode:
        u64,
}
