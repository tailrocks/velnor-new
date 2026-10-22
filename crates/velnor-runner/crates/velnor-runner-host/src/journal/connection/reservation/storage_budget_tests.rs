use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use super::storage_budget::{
    StorageAdmission, StorageAdmissionSample, StorageFilesystemKind, StorageInodePolicy,
    StorageMedium, StorageMemoryObservation, StoragePoolLimit, StoragePoolObservation,
    StorageProfile, StorageProvisioningAdmission, StorageProvisioningMemoryObservation,
    StorageReservationPlan, StorageRole, StorageRoleCharge, StorageRoleLimit,
    StorageSampleIdentity,
};
use crate::error::HostError;
use crate::launch::StartPermit;

pub(crate) fn fixture_profile() -> StorageProfile {
    StorageProfile::new(
        "a".repeat(32),
        vec![
            StoragePoolLimit::new("c".repeat(64), StorageMedium::Disk, 16_384, 256, 128, 4)
                .expect("pool limit"),
        ],
        role_limits(StorageMedium::Disk, 12_000),
    )
    .expect("fixture profile")
}

pub(crate) fn fixture_provisioning_profile() -> StorageProfile {
    let disk_pool = "1".repeat(64);
    let tmpfs = fixture_provisioning_tmpfs_limits();
    let pools = fixture_provisioning_pools(&disk_pool, &tmpfs);
    let roles = fixture_provisioning_roles(&disk_pool, &tmpfs);
    StorageProfile::new("b".repeat(32), pools, roles).expect("provisioning profile")
}

fn fixture_provisioning_tmpfs_limits() -> [(StorageRole, u64, u64); 8] {
    [
        (StorageRole::RunnerHome, 2_u64 << 30, 131_072),
        (StorageRole::RunnerWork, 4_u64 << 30, 262_144),
        (StorageRole::RunnerTmp, 1_u64 << 30, 65_536),
        (StorageRole::RunnerShm, 16_u64 << 20, 1_024),
        (StorageRole::SharedRun, 64_u64 << 20, 8_192),
        (StorageRole::DindData, 2_u64 << 30, 131_072),
        (StorageRole::DindTmp, 1_u64 << 30, 65_536),
        (StorageRole::DindShm, 16_u64 << 20, 1_024),
    ]
}

fn fixture_provisioning_pools(
    disk_pool: &str,
    tmpfs: &[(StorageRole, u64, u64); 8],
) -> Vec<StoragePoolLimit> {
    let mut pools = vec![
        StoragePoolLimit::new_with_inode_policy(
            disk_pool.to_owned(),
            StorageMedium::Disk,
            StorageInodePolicy::DynamicBtrfs,
            16 * 1024 * 1024 * 1024,
            0,
            2 * 1024 * 1024 * 1024,
            0,
        )
        .expect("disk pool"),
    ];
    for (index, (_, max_bytes, max_inodes)) in tmpfs.iter().enumerate() {
        pools.push(
            StoragePoolLimit::new(
                format!("{:x}", index + 2).repeat(64),
                StorageMedium::Tmpfs,
                *max_bytes,
                *max_inodes,
                0,
                0,
            )
            .expect("tmpfs pool"),
        );
    }
    pools
}

fn fixture_provisioning_roles(
    disk_pool: &str,
    tmpfs: &[(StorageRole, u64, u64); 8],
) -> Vec<StorageRoleLimit> {
    let mut roles = Vec::with_capacity(StorageRole::ALL.len());
    for role in [
        StorageRole::RunnerRoot,
        StorageRole::DindRoot,
        StorageRole::EngineMetadata,
    ] {
        let bytes = if role == StorageRole::EngineMetadata {
            2_u64 << 30
        } else {
            1_u64 << 30
        };
        roles.push(
            StorageRoleLimit::new(
                role,
                disk_pool.to_owned(),
                StorageMedium::Disk,
                bytes,
                10_000,
                0,
            )
            .expect("disk role"),
        );
    }
    for (index, (role, max_bytes, max_inodes)) in tmpfs.iter().copied().enumerate() {
        roles.push(
            StorageRoleLimit::new(
                role,
                format!("{:x}", index + 2).repeat(64),
                StorageMedium::Tmpfs,
                max_bytes,
                max_inodes,
                max_bytes,
            )
            .expect("tmpfs role"),
        );
    }
    let work_pool = format!("{:x}", 3).repeat(64);
    roles.push(
        StorageRoleLimit::new_alias(
            StorageRole::CacheLive,
            work_pool.clone(),
            StorageMedium::Tmpfs,
            StorageRole::RunnerWork,
            512_u64 << 20,
            32_768,
        )
        .expect("live cache alias"),
    );
    roles.push(
        StorageRoleLimit::new_alias(
            StorageRole::CacheStage,
            work_pool,
            StorageMedium::Tmpfs,
            StorageRole::RunnerWork,
            512_u64 << 20,
            32_768,
        )
        .expect("staging cache alias"),
    );
    roles
}

pub(crate) fn fixture_storage_provisioning_admission(
    permit: &StartPermit,
) -> Result<StorageProvisioningAdmission, HostError> {
    fixture_storage_provisioning_admission_with_filesystem_digest(permit, "a".repeat(64))
}

pub(crate) fn fixture_storage_provisioning_admission_with_filesystem_digest(
    permit: &StartPermit,
    filesystem_instance_digest: String,
) -> Result<StorageProvisioningAdmission, HostError> {
    let profile = fixture_provisioning_profile();
    fixture_storage_provisioning_admission_for_profile(permit, filesystem_instance_digest, &profile)
}

pub(crate) fn fixture_storage_provisioning_admission_for_profile(
    permit: &StartPermit,
    filesystem_instance_digest: String,
    profile: &StorageProfile,
) -> Result<StorageProvisioningAdmission, HostError> {
    let sample = StorageAdmissionSample::new(
        StorageSampleIdentity::new(
            permit.sample_generation_id().to_owned(),
            profile.generation_id().to_owned(),
            profile.profile_digest().to_owned(),
            permit.engine_id().to_owned(),
            permit.docker_root_digest().to_owned(),
        )?,
        Instant::now(),
        vec![StoragePoolObservation::new_with_filesystem(
            "1".repeat(64),
            filesystem_instance_digest,
            StorageMedium::Disk,
            StorageFilesystemKind::Btrfs,
            64_u64 << 30,
            60_u64 << 30,
            0,
            0,
        )?],
        None,
    )?;
    Ok(StorageProvisioningAdmission::new(
        sample,
        StorageProvisioningMemoryObservation::new(64_u64 << 30, 32_u64 << 30)?,
    ))
}

#[test]
fn provisioning_memory_uses_guest_total_and_available_only() -> Result<(), HostError> {
    let permit = StartPermit::test_fixture();
    let admission = fixture_storage_provisioning_admission(&permit)?;
    let profile = fixture_provisioning_profile();
    let required_memory = 8_u64 << 30;
    let now = Instant::now();

    StorageProvisioningAdmission::new(
        admission.sample().clone(),
        StorageProvisioningMemoryObservation::new(64_u64 << 30, 24_u64 << 30)?,
    )
    .validate_for(
        &profile,
        permit.sample_generation_id(),
        permit.engine_id(),
        permit.docker_root_digest(),
        required_memory,
        now,
    )?;

    let insufficient_available = StorageProvisioningAdmission::new(
        admission.sample().clone(),
        StorageProvisioningMemoryObservation::new(64_u64 << 30, 8_u64 << 30)?,
    );
    assert!(
        insufficient_available
            .validate_for(
                &profile,
                permit.sample_generation_id(),
                permit.engine_id(),
                permit.docker_root_digest(),
                required_memory,
                now,
            )
            .is_err()
    );

    let insufficient_total = StorageProvisioningAdmission::new(
        admission.sample().clone(),
        StorageProvisioningMemoryObservation::new(8_u64 << 30, 8_u64 << 30)?,
    );
    assert!(
        insufficient_total
            .validate_for(
                &profile,
                permit.sample_generation_id(),
                permit.engine_id(),
                permit.docker_root_digest(),
                required_memory,
                now,
            )
            .is_err()
    );
    Ok(())
}

#[test]
fn provisioning_fixture_matches_the_fixed_host_contract() {
    let profile = fixture_provisioning_profile();
    profile
        .validate_provisioning_contract()
        .expect("full provisioning profile");
}

#[test]
fn provisioning_profile_charges_home_inbox_bytes_and_entries() {
    let profile = fixture_provisioning_profile();
    let home_index = profile
        .roles()
        .iter()
        .position(|role| role.role() == StorageRole::RunnerHome)
        .expect("RunnerHome role");
    let home = &profile.roles()[home_index];

    for (max_bytes, max_inodes, retained_memory_bytes) in [
        (32_767, home.max_inodes(), 32_767),
        (home.max_bytes(), 4, home.retained_memory_bytes()),
    ] {
        let mut roles = profile.roles().to_vec();
        roles[home_index] = StorageRoleLimit::new(
            StorageRole::RunnerHome,
            home.pool_id().to_owned(),
            home.medium(),
            max_bytes,
            max_inodes,
            retained_memory_bytes,
        )
        .expect("valid but undersized Home role");
        let undersized = StorageProfile::new("b".repeat(32), profile.pools().to_vec(), roles)
            .expect("structurally valid profile");
        assert!(
            undersized.validate_provisioning_contract().is_err(),
            "RunnerHome must cover both inbox bytes and filesystem entries"
        );
    }
}

pub(crate) fn fixture_admission(permit: &StartPermit) -> Result<StorageAdmission, HostError> {
    let profile = fixture_profile();
    let filesystem_instance_digest = "d".repeat(64);
    let roles = profile
        .roles()
        .iter()
        .filter(|limit| limit.alias_of().is_none())
        .map(|limit| {
            let (charge_bytes, charge_inodes) = if limit.role() == StorageRole::RunnerWork {
                (1024, 16)
            } else {
                (1, 1)
            };
            StorageRoleCharge::new(
                limit.role(),
                limit.pool_id().to_owned(),
                filesystem_instance_digest.clone(),
                StorageMedium::Disk,
                charge_bytes,
                charge_inodes,
                0,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    let plan = StorageReservationPlan::new(
        profile.generation_id().to_owned(),
        profile.profile_digest().to_owned(),
        "1".repeat(64),
        roles,
    )?;
    let identity = StorageSampleIdentity::new(
        permit.sample_generation_id().to_owned(),
        profile.generation_id().to_owned(),
        profile.profile_digest().to_owned(),
        permit.engine_id().to_owned(),
        permit.docker_root_digest().to_owned(),
    )?;
    let sample = StorageAdmissionSample::new(
        identity,
        Instant::now(),
        vec![StoragePoolObservation::new(
            "c".repeat(64),
            filesystem_instance_digest,
            StorageMedium::Disk,
            32_768,
            32_768,
            512,
            512,
        )?],
        None,
    )?;
    StorageAdmission::new(plan, sample)
}

#[path = "storage_budget_tests/helpers.rs"]
mod helpers;
#[path = "storage_budget_tests/spec.rs"]
mod spec;

use helpers::role_limits;
