use std::time::Instant;

use super::storage_budget::{
    StorageAdmissionSample, StorageFilesystemKind, StorageInodePolicy, StorageMedium,
    StoragePoolLimit, StoragePoolObservation, StoragePoolObservationInput, StorageProfile,
    StorageReservationPlan, StorageRole, StorageRoleCharge, StorageRoleLimit,
    StorageSampleIdentity,
};

fn sample_inputs() -> (
    StorageProfile,
    StorageReservationPlan,
    StorageSampleIdentity,
) {
    let profile = StorageProfile::new(
        "a".repeat(32),
        vec![
            StoragePoolLimit::new("c".repeat(64), StorageMedium::Disk, 16_384, 256, 128, 4)
                .expect("pool"),
        ],
        role_limits(StorageMedium::Disk, 12_000),
    )
    .expect("profile");
    let plan = plan(&profile);
    let identity = StorageSampleIdentity::new(
        "e".repeat(32),
        profile.generation_id().to_owned(),
        profile.profile_digest().to_owned(),
        "test-engine".to_owned(),
        "f".repeat(64),
    )
    .expect("identity");
    (profile, plan, identity)
}

fn role_limits(medium: StorageMedium, work_bytes: u64) -> Vec<StorageRoleLimit> {
    StorageRole::ALL
        .into_iter()
        .map(|role| match role {
            StorageRole::CacheLive | StorageRole::CacheStage => StorageRoleLimit::new_alias(
                role,
                "c".repeat(64),
                medium,
                StorageRole::RunnerWork,
                128,
                2,
            ),
            StorageRole::RunnerWork => {
                StorageRoleLimit::new(role, "c".repeat(64), medium, work_bytes, 200, 0)
            }
            _ => StorageRoleLimit::new(role, "c".repeat(64), medium, 16, 1, 0),
        })
        .collect::<Result<Vec<_>, _>>()
        .expect("role limits")
}

fn plan(profile: &StorageProfile) -> StorageReservationPlan {
    let roles = profile
        .roles()
        .iter()
        .filter(|limit| limit.alias_of().is_none())
        .map(|limit| {
            let (bytes, inodes) = if limit.role() == StorageRole::RunnerWork {
                (1024, 16)
            } else {
                (1, 1)
            };
            StorageRoleCharge::new(
                limit.role(),
                limit.pool_id().to_owned(),
                "d".repeat(64),
                limit.medium(),
                bytes,
                inodes,
                0,
            )
        })
        .collect::<Result<Vec<_>, _>>()
        .expect("physical role charges");
    StorageReservationPlan::new(
        profile.generation_id().to_owned(),
        profile.profile_digest().to_owned(),
        "1".repeat(64),
        roles,
    )
    .expect("plan")
}

#[test]
fn unsupported_inode_stat_is_not_a_zero_capacity_sample() {
    let sample = StoragePoolObservation::new(
        "c".repeat(64),
        "d".repeat(64),
        StorageMedium::Disk,
        8192,
        8192,
        0,
        0,
    )
    .expect("raw stat sample");
    assert_eq!(sample.total_inodes(), 0);
    assert_eq!(sample.available_inodes(), 0);
}

#[test]
fn dynamic_btrfs_inode_policy_requires_btrfs_and_keeps_byte_budget() {
    let pool = StoragePoolLimit::new_with_inode_policy(
        "c".repeat(64),
        StorageMedium::Disk,
        StorageInodePolicy::DynamicBtrfs,
        16_384,
        0,
        128,
        0,
    )
    .expect("dynamic pool");
    let profile = StorageProfile::new(
        "a".repeat(32),
        vec![pool],
        role_limits(StorageMedium::Disk, 12_000),
    )
    .expect("dynamic profile");
    let plan = plan(&profile);
    let sample = StorageAdmissionSample::new(
        StorageSampleIdentity::new(
            "e".repeat(32),
            profile.generation_id().to_owned(),
            profile.profile_digest().to_owned(),
            "test-engine".to_owned(),
            "f".repeat(64),
        )
        .expect("identity"),
        Instant::now(),
        vec![
            StoragePoolObservation::new_with_filesystem(StoragePoolObservationInput {
                pool_id: "c".repeat(64),
                filesystem_instance_digest: "d".repeat(64),
                medium: StorageMedium::Disk,
                filesystem_kind: StorageFilesystemKind::Btrfs,
                total_bytes: 32_768,
                available_bytes: 32_768,
                total_inodes: 0,
                available_inodes: 0,
            })
            .expect("btrfs sample"),
        ],
        None,
    )
    .expect("sample");
    assert!(
        sample
            .validate_for(
                &profile,
                &plan,
                &StorageAdmissionSample::validation_identity(
                    &"e".repeat(32),
                    "test-engine",
                    &"f".repeat(64),
                ),
                2048,
                Instant::now(),
            )
            .is_ok()
    );
    assert_eq!(sample.pools()[0].available_inodes(), 0);
}

#[test]
fn sample_for_a_different_filesystem_instance_is_rejected() {
    let (profile, plan, identity) = sample_inputs();
    let sample = StorageAdmissionSample::new(
        identity,
        Instant::now(),
        vec![
            StoragePoolObservation::new(
                "c".repeat(64),
                "0".repeat(64),
                StorageMedium::Disk,
                32_768,
                32_768,
                256,
                256,
            )
            .expect("observation"),
        ],
        None,
    )
    .expect("sample");
    assert!(
        sample
            .validate_for(
                &profile,
                &plan,
                &StorageAdmissionSample::validation_identity(
                    &"e".repeat(32),
                    "test-engine",
                    &"f".repeat(64),
                ),
                2048,
                Instant::now(),
            )
            .is_err()
    );
}
