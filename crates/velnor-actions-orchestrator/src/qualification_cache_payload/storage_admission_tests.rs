use super::{
    ROLE_NAMES, VerifiedCacheBudget, canonical_instance_binding_digest,
    canonical_projection_digest, parse_projection,
};
use crate::qualification_cache_payload::storage_admission_types::{
    CacheLimitsV1, CgroupV1, EngineV1, InstanceBindingV1, JobFactsMountV1, ProfileV1,
    ReservationV1, RunnerReferenceV1, RunnerV1, SampleV1, ScopeV1, StorageCacheAdmissionV1,
    StorageRoleV1,
};
use std::path::Path;

fn digest(byte: char) -> String {
    byte.to_string().repeat(64)
}

fn role(name: &str, charge_bytes: u64, charge_inodes: u64) -> StorageRoleV1 {
    StorageRoleV1 {
        role: name.to_owned(),
        pool_id: "runner-work-pool".to_owned(),
        filesystem_instance_digest: digest('a'),
        medium: "tmpfs".to_owned(),
        filesystem_kind: "tmpfs".to_owned(),
        mount_target: "/home/runner/_work".to_owned(),
        mount_id: 7,
        device_major: 0,
        device_minor: 44,
        statx_device_major: 0,
        statx_device_minor: 44,
        fs_type_magic: 0x0102_1994,
        filesystem_id: 4,
        total_bytes: 10_000,
        free_bytes: 9_000,
        available_bytes: 8_000,
        inode_total: Some(8_000),
        inode_free: Some(7_000),
        inode_available: Some(6_000),
        inode_policy: "finite".to_owned(),
        charge_bytes,
        charge_inodes,
        retained_memory_bytes: 0,
        owned_usage_bytes: 10,
        owned_usage_inodes: 1,
    }
}

fn role_rows() -> Vec<StorageRoleV1> {
    let mut roles = ROLE_NAMES
        .iter()
        .map(|name| role(name, 100, 100))
        .collect::<Vec<_>>();
    roles[2] = role("runner-work", 10_000_000, 1_000_000);
    roles[10] = role("cache-live", 4_000_000, 400_000);
    roles[11] = role("cache-stage", 5_000_000, 500_000);
    roles
}

fn cgroups() -> Vec<CgroupV1> {
    vec![
        CgroupV1 {
            role: "dind".to_owned(),
            container_id: "dind-container".to_owned(),
            path: "/sys/fs/cgroup/dind".to_owned(),
            memory_max_bytes: Some(4_096),
            memory_current_bytes: 1_024,
            cpu_quota_micros: Some(100_000),
            cpu_period_micros: 100_000,
            pids_max: Some(256),
        },
        CgroupV1 {
            role: "runner".to_owned(),
            container_id: "runner-container".to_owned(),
            path: "/sys/fs/cgroup/runner".to_owned(),
            memory_max_bytes: Some(8_192),
            memory_current_bytes: 2_048,
            cpu_quota_micros: Some(200_000),
            cpu_period_micros: 100_000,
            pids_max: Some(512),
        },
    ]
}

fn instance_binding(roles: Vec<StorageRoleV1>) -> InstanceBindingV1 {
    InstanceBindingV1 {
        schema_version: 1,
        connection_id: "connection-7".to_owned(),
        scale_set_id: 5,
        reservation_id: 11,
        intent_id: 13,
        engine_id: "engine-3".to_owned(),
        docker_root_digest: digest('c'),
        sample_generation_id: "sample-1".to_owned(),
        sample_observed_at_unix_ms: 1,
        profile_generation_id: "profile-2".to_owned(),
        profile_digest: digest('d'),
        guest_boot_id: "boot-id".to_owned(),
        runner_container_id: "runner-container".to_owned(),
        dind_container_id: "dind-container".to_owned(),
        roles,
        cgroups: cgroups(),
    }
}

pub(super) fn document() -> StorageCacheAdmissionV1 {
    let mut value = StorageCacheAdmissionV1 {
        schema: "velnor.storage-cache-admission".to_owned(),
        version: 1,
        scope: ScopeV1 {
            connection_id: "connection-7".to_owned(),
            scale_set_id: 5,
            binding_digest: digest('b'),
        },
        reservation: ReservationV1 {
            id: 11,
            launch_intent_id: 13,
        },
        runner: RunnerV1 {
            reference: RunnerReferenceV1 {
                id: 17,
                name: "runner-17".to_owned(),
                runner_scale_set_id: 5,
            },
            container_id: "runner-container".to_owned(),
            dind_container_id: "dind-container".to_owned(),
        },
        engine: EngineV1 {
            id: "engine-3".to_owned(),
            docker_root_digest: digest('c'),
        },
        profile: ProfileV1 {
            generation_id: "profile-2".to_owned(),
            digest: digest('d'),
        },
        sample: SampleV1 {
            generation_id: "sample-1".to_owned(),
            observed_at_unix_ms: 1,
        },
        instance_binding: instance_binding(role_rows()),
        instance_binding_digest: String::new(),
        guest_image_sha256: digest('e'),
        cache: CacheLimitsV1 {
            live_path: "/tmp/runner/velnor/cache".to_owned(),
            stage_path: "/tmp/runner/velnor/cache-staging".to_owned(),
            live_bytes: 4_000_000,
            live_inodes: 400_000,
            stage_bytes: 5_000_000,
            stage_inodes: 500_000,
        },
        job_facts_mount: JobFactsMountV1 {
            target: "/opt/velnor/job-facts".to_owned(),
            read_only: true,
            host_directory_device: 3,
            host_directory_inode: 4,
            guest_mount_id: 5,
            guest_device_major: 0,
            guest_device_minor: 11,
            guest_directory_device: 12,
            guest_directory_inode: 13,
        },
        projection_digest: String::new(),
    };
    value.instance_binding_digest =
        canonical_instance_binding_digest(&value.instance_binding).expect("binding digest");
    value.projection_digest = canonical_projection_digest(&value).expect("projection digest");
    value
}

fn bytes(value: &StorageCacheAdmissionV1) -> Vec<u8> {
    serde_json::to_vec(value).expect("serialize projection")
}

pub(super) fn parsed_budget(value: &StorageCacheAdmissionV1) -> VerifiedCacheBudget {
    parse_projection(&bytes(value), Path::new("/tmp/runner"), "runner-17")
        .expect("valid projection")
}

#[test]
fn exact_projection_binds_the_job_and_old_samples_do_not_expire_retained_authority() {
    let value = document();
    let budget = parsed_budget(&value);
    assert_eq!(budget.limits(), (4_000_000, 400_000, 5_000_000, 500_000));
    assert_eq!(
        budget.operation_limit(4 * 1024 * 1024 * 1024, 100, 10, 200, 20),
        Ok(3_999_900)
    );
}

#[test]
fn restore_budget_counts_compressed_archive_and_live_import_growth() {
    let budget = parsed_budget(&document());
    let compressed_archive_bytes = 1_000_000;
    let fresh = super::measure::FreshCapacity {
        available_bytes: 100_000_000,
        available_inodes: Some(100_000),
        fragment_bytes: 4_096,
    };
    let limits = budget
        .restore_limits(
            4 * 1024 * 1024,
            compressed_archive_bytes,
            super::measure::CacheTreeUsage {
                bytes: 1_000,
                inodes: 10,
            },
            super::measure::CacheTreeUsage {
                bytes: 2_000,
                inodes: 20,
            },
            fresh,
        )
        .expect("bounded warm restore");
    assert_eq!(limits.max_download_bytes(), 4_986_634);
    assert!(limits.raw_stage_bytes() < 4 * 1024 * 1024);
    assert!(limits.live_growth_bytes() <= fresh.available_bytes);
}

#[test]
fn restore_budget_rejects_archive_download_before_action_when_scratch_is_short() {
    let budget = parsed_budget(&document());
    let fresh = super::measure::FreshCapacity {
        available_bytes: 2_000_000,
        available_inodes: Some(100_000),
        fragment_bytes: 4_096,
    };
    assert_eq!(
        budget.restore_limits(
            4 * 1024 * 1024,
            1_990_000,
            super::measure::CacheTreeUsage {
                bytes: 1_000,
                inodes: 10,
            },
            super::measure::CacheTreeUsage {
                bytes: 2_000,
                inodes: 20,
            },
            fresh,
        ),
        Err("cache_storage_download_budget_exceeded")
    );
}

#[test]
fn restore_budget_rejects_small_stage_remainder_without_subtraction_underflow() {
    let budget = parsed_budget(&document());
    let fresh = super::measure::FreshCapacity {
        available_bytes: 100_000_000,
        available_inodes: Some(100_000),
        fragment_bytes: 4_096,
    };
    assert_eq!(
        budget.restore_limits(
            4 * 1024 * 1024,
            1_000,
            super::measure::CacheTreeUsage {
                bytes: 1_000,
                inodes: 10,
            },
            super::measure::CacheTreeUsage {
                bytes: 4_992_000,
                inodes: 20,
            },
            fresh,
        ),
        Err("cache_storage_restore_capacity_insufficient")
    );
}

#[test]
fn restore_budget_rejects_live_growth_and_inode_shortfalls() {
    let mut value = document();
    value.cache.live_bytes = 10_000;
    value.cache.live_inodes = 1_000;
    value.instance_binding.roles[10].charge_bytes = 10_000;
    value.instance_binding.roles[10].charge_inodes = 1_000;
    value.instance_binding.roles[2].charge_bytes = 10_015_000;
    value.instance_binding.roles[2].charge_inodes = 1_000_000;
    value.instance_binding_digest =
        canonical_instance_binding_digest(&value.instance_binding).expect("binding digest");
    value.projection_digest = canonical_projection_digest(&value).expect("projection digest");
    let budget = parsed_budget(&value);
    let fresh = super::measure::FreshCapacity {
        available_bytes: 100_000_000,
        available_inodes: Some(100_000),
        fragment_bytes: 4_096,
    };
    assert_eq!(
        budget.restore_limits(
            4 * 1024 * 1024,
            1_000,
            super::measure::CacheTreeUsage {
                bytes: 9_500,
                inodes: 900,
            },
            super::measure::CacheTreeUsage {
                bytes: 2_000,
                inodes: 20,
            },
            fresh,
        ),
        Err("cache_storage_restore_capacity_insufficient")
    );
}

#[test]
fn projection_rejects_runner_and_path_substitution() {
    let value = document();
    assert_eq!(
        parse_projection(&bytes(&value), Path::new("/tmp/runner"), "runner-other").err(),
        Some("cache_storage_runner_binding_mismatch")
    );
    assert_eq!(
        parse_projection(&bytes(&value), Path::new("/tmp/elsewhere"), "runner-17").err(),
        Some("cache_storage_cache_path_or_limit_invalid")
    );
}

#[test]
fn guest_image_sha256_is_validated_and_bound_into_the_projection_digest() {
    let value = document();
    let mut changed = value.clone();
    changed.guest_image_sha256 = digest('f');
    assert_ne!(
        canonical_projection_digest(&value).expect("projection digest"),
        canonical_projection_digest(&changed).expect("changed projection digest")
    );

    let mut invalid = value;
    invalid.guest_image_sha256 = "G".repeat(64);
    invalid.projection_digest = canonical_projection_digest(&invalid).expect("projection digest");
    assert_eq!(
        parse_projection(&bytes(&invalid), Path::new("/tmp/runner"), "runner-17").err(),
        Some("cache_storage_digest_invalid")
    );
}

#[test]
fn projection_rejects_unknown_duplicate_noncanonical_and_bad_digest_fields() {
    let value = document();
    let mut unknown = serde_json::to_value(&value).expect("value");
    unknown["extra"] = serde_json::json!(true);
    assert_eq!(
        parse_projection(
            &serde_json::to_vec(&unknown).expect("serialize"),
            Path::new("/tmp/runner"),
            "runner-17"
        )
        .err(),
        Some("cache_storage_projection_invalid")
    );

    let mut duplicate = bytes(&value);
    let tail = duplicate.pop().expect("closing object");
    assert_eq!(tail, b'}');
    duplicate.extend_from_slice(br#","version":1}"#);
    assert_eq!(
        parse_projection(&duplicate, Path::new("/tmp/runner"), "runner-17").err(),
        Some("cache_storage_projection_invalid")
    );

    let mut noncanonical = bytes(&value);
    noncanonical.insert(0, b' ');
    assert_eq!(
        parse_projection(&noncanonical, Path::new("/tmp/runner"), "runner-17").err(),
        Some("cache_storage_projection_noncanonical")
    );

    let mut changed = value;
    changed.job_facts_mount.read_only = false;
    assert_eq!(
        parse_projection(&bytes(&changed), Path::new("/tmp/runner"), "runner-17").err(),
        Some("cache_storage_job_facts_mount_invalid")
    );
}

#[test]
fn projection_rejects_cross_binding_and_alias_limit_changes() {
    let mut changed = document();
    changed.instance_binding.sample_observed_at_unix_ms += 1;
    changed.instance_binding_digest =
        canonical_instance_binding_digest(&changed.instance_binding).expect("digest");
    changed.projection_digest = canonical_projection_digest(&changed).expect("digest");
    assert_eq!(
        parse_projection(&bytes(&changed), Path::new("/tmp/runner"), "runner-17").err(),
        Some("cache_storage_projection_binding_invalid")
    );

    let mut changed = document();
    changed.cache.stage_bytes -= 1;
    changed.projection_digest = canonical_projection_digest(&changed).expect("digest");
    assert_eq!(
        parse_projection(&bytes(&changed), Path::new("/tmp/runner"), "runner-17").err(),
        Some("cache_storage_alias_limits_invalid")
    );
}

#[test]
fn budget_refuses_live_stage_and_inode_overruns() {
    let budget = parsed_budget(&document());
    assert_eq!(
        budget.operation_limit(4_096, 4_000_001, 10, 1, 1),
        Err("cache_storage_live_budget_exceeded")
    );
    assert_eq!(
        budget.operation_limit(4_096, 1, 1, 5_000_001, 1),
        Err("cache_storage_stage_budget_exceeded")
    );
    assert_eq!(
        budget.operation_limit(4_096, 1, 400_001, 1, 1),
        Err("cache_storage_inode_budget_exceeded")
    );
}
