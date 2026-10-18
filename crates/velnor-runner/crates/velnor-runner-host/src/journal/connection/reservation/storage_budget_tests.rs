use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use super::storage_budget::{
    StorageAdmission, StorageAdmissionSample, StorageMedium, StorageMemoryObservation,
    StoragePoolLimit, StoragePoolObservation, StorageProfile, StorageReservationPlan, StorageRole,
    StorageRoleCharge, StorageRoleLimit, StorageSampleIdentity,
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

fn profile(medium: StorageMedium) -> StorageProfile {
    StorageProfile::new(
        "a".repeat(32),
        vec![
            StoragePoolLimit::new("c".repeat(64), medium, 16_384, 256, 128, 4).expect("pool limit"),
        ],
        role_limits(medium, 12_000),
    )
    .expect("profile")
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
            StorageRole::RunnerWork => StorageRoleLimit::new(
                role,
                "c".repeat(64),
                medium,
                work_bytes,
                200,
                if medium == StorageMedium::Tmpfs {
                    work_bytes
                } else {
                    0
                },
            ),
            _ => StorageRoleLimit::new(
                role,
                "c".repeat(64),
                medium,
                16,
                1,
                if medium == StorageMedium::Tmpfs {
                    16
                } else {
                    0
                },
            ),
        })
        .collect::<Result<Vec<_>, _>>()
        .expect("role limits")
}

fn role(medium: StorageMedium) -> StorageRoleCharge {
    let charge = 1024;
    StorageRoleCharge::new(
        StorageRole::RunnerWork,
        "c".repeat(64),
        "d".repeat(64),
        medium,
        charge,
        16,
        if medium == StorageMedium::Tmpfs {
            charge
        } else {
            0
        },
    )
    .expect("role charge")
}

fn sample(profile: &StorageProfile, memory: bool, observed_at: Instant) -> StorageAdmissionSample {
    StorageAdmissionSample::new(
        StorageSampleIdentity::new(
            "e".repeat(32),
            profile.generation_id().to_owned(),
            profile.profile_digest().to_owned(),
            "test-engine".to_owned(),
            "f".repeat(64),
        )
        .expect("sample identity"),
        observed_at,
        vec![
            StoragePoolObservation::new(
                "c".repeat(64),
                "d".repeat(64),
                profile.pools()[0].medium(),
                32_768,
                32_768,
                512,
                512,
            )
            .expect("pool sample"),
        ],
        memory.then(|| StorageMemoryObservation::new(16_384, 32_768, 4096, 0, 0).expect("memory")),
    )
    .expect("sample")
}

fn plan(profile: &StorageProfile, medium: StorageMedium) -> StorageReservationPlan {
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
                "d".repeat(64),
                medium,
                charge_bytes,
                charge_inodes,
                if medium == StorageMedium::Tmpfs {
                    charge_bytes
                } else {
                    0
                },
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
fn closed_roles_have_exact_storage_names() {
    let roles = [
        StorageRole::RunnerRoot,
        StorageRole::RunnerHome,
        StorageRole::RunnerWork,
        StorageRole::RunnerTmp,
        StorageRole::RunnerShm,
        StorageRole::SharedRun,
        StorageRole::DindRoot,
        StorageRole::DindData,
        StorageRole::DindTmp,
        StorageRole::DindShm,
        StorageRole::CacheLive,
        StorageRole::CacheStage,
        StorageRole::EngineMetadata,
    ];
    let names = [
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
    for (role, name) in roles.into_iter().zip(names) {
        assert_eq!(role.as_str(), name);
        assert_eq!(StorageRole::parse(name).expect("known role"), role);
    }
    assert!(StorageRole::parse("runner-cache-alias").is_err());
}

#[test]
fn profile_rejects_duplicate_pools_and_invalid_capacity() {
    let pool = StoragePoolLimit::new("c".repeat(64), StorageMedium::Disk, 16_384, 256, 128, 4)
        .expect("pool");
    let roles = role_limits(StorageMedium::Disk, 12_000);
    assert!(StorageProfile::new("a".repeat(32), vec![pool.clone(), pool], roles.clone()).is_err());
    let mut duplicate_roles = roles;
    duplicate_roles.push(duplicate_roles[0].clone());
    assert!(
        StorageProfile::new(
            "a".repeat(32),
            vec![
                StoragePoolLimit::new("c".repeat(64), StorageMedium::Disk, 16_384, 256, 128, 4)
                    .expect("pool")
            ],
            duplicate_roles,
        )
        .is_err()
    );
    assert!(StoragePoolLimit::new("c".repeat(64), StorageMedium::Disk, 10, 10, 11, 0).is_err());
    assert!(StoragePoolLimit::new("c".repeat(64), StorageMedium::Disk, 0, 10, 0, 0).is_err());
}

#[test]
fn plan_rejects_duplicate_physical_roles_and_wrong_medium_memory_charge() {
    let disk = role(StorageMedium::Disk);
    assert!(
        StorageReservationPlan::new(
            "a".repeat(32),
            "b".repeat(64),
            "1".repeat(64),
            vec![disk.clone(), disk],
        )
        .is_err()
    );
    assert!(
        StorageRoleCharge::new(
            StorageRole::RunnerWork,
            "c".repeat(64),
            "d".repeat(64),
            StorageMedium::Disk,
            10,
            1,
            1,
        )
        .is_err()
    );
}

#[test]
fn profile_digest_is_order_independent_and_binds_limits() {
    let first = profile(StorageMedium::Disk);
    let second = profile(StorageMedium::Disk);
    assert_eq!(first.profile_digest(), second.profile_digest());
    let changed = StorageProfile::new(
        "a".repeat(32),
        vec![
            StoragePoolLimit::new("c".repeat(64), StorageMedium::Disk, 16_384, 256, 128, 4)
                .expect("pool limit"),
        ],
        role_limits(StorageMedium::Disk, 11_999),
    )
    .expect("profile");
    assert_ne!(first.profile_digest(), changed.profile_digest());
}

#[test]
fn fresh_sample_must_match_engine_profile_pool_and_instance() {
    let profile = profile(StorageMedium::Disk);
    let plan = plan(&profile, StorageMedium::Disk);
    let admission = StorageAdmission::new(plan.clone(), sample(&profile, false, Instant::now()))
        .expect("admission");
    assert!(
        admission
            .sample()
            .validate_for(
                &profile,
                &plan,
                &"e".repeat(32),
                "test-engine",
                &"f".repeat(64),
                2048,
                Instant::now(),
            )
            .is_ok()
    );
    assert!(
        admission
            .sample()
            .validate_for(
                &profile,
                &plan,
                &"0".repeat(32),
                "test-engine",
                &"f".repeat(64),
                2048,
                Instant::now(),
            )
            .is_err()
    );
    assert!(
        admission
            .sample()
            .validate_for(
                &profile,
                &plan,
                &"e".repeat(32),
                "other-engine",
                &"f".repeat(64),
                2048,
                Instant::now(),
            )
            .is_err()
    );
    assert!(
        admission
            .sample()
            .validate_for(
                &profile,
                &plan,
                &"e".repeat(32),
                "test-engine",
                &"0".repeat(64),
                2048,
                Instant::now(),
            )
            .is_err()
    );
    assert!(
        admission
            .sample()
            .validate_for(
                &profile,
                &plan,
                &"e".repeat(32),
                "test-engine",
                &"f".repeat(64),
                2048,
                Instant::now() + Duration::from_secs(31),
            )
            .is_err()
    );
}

#[test]
fn persisted_sample_wall_time_must_be_recent_and_not_future() {
    let profile = profile(StorageMedium::Disk);
    let plan = plan(&profile, StorageMedium::Disk);
    let now_ms = u64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("wall clock")
            .as_millis(),
    )
    .expect("wall milliseconds");
    let pools = vec![
        StoragePoolObservation::new(
            "c".repeat(64),
            "d".repeat(64),
            StorageMedium::Disk,
            32_768,
            32_768,
            512,
            512,
        )
        .expect("pool sample"),
    ];
    for observed_at_unix_ms in [now_ms - 31_000, now_ms + 1_000] {
        let sample = StorageAdmissionSample::new_at(
            StorageSampleIdentity::new(
                "e".repeat(32),
                profile.generation_id().to_owned(),
                profile.profile_digest().to_owned(),
                "test-engine".to_owned(),
                "f".repeat(64),
            )
            .expect("sample identity"),
            Instant::now(),
            observed_at_unix_ms,
            pools.clone(),
            None,
        )
        .expect("well-formed sample row");
        assert!(
            sample
                .validate_for(
                    &profile,
                    &plan,
                    &"e".repeat(32),
                    "test-engine",
                    &"f".repeat(64),
                    2048,
                    Instant::now(),
                )
                .is_err()
        );
    }
}

#[test]
fn tmpfs_charge_requires_memory_headroom_observation() {
    let profile = profile(StorageMedium::Tmpfs);
    let plan = plan(&profile, StorageMedium::Tmpfs);
    let without_memory = StorageAdmissionSample::new(
        StorageSampleIdentity::new(
            "e".repeat(32),
            profile.generation_id().to_owned(),
            profile.profile_digest().to_owned(),
            "test-engine".to_owned(),
            "f".repeat(64),
        )
        .expect("sample identity"),
        Instant::now(),
        vec![
            StoragePoolObservation::new(
                "c".repeat(64),
                "d".repeat(64),
                StorageMedium::Tmpfs,
                8192,
                8192,
                256,
                256,
            )
            .expect("pool sample"),
        ],
        None,
    )
    .expect("sample without optional memory details");
    let admission = StorageAdmission::new(plan.clone(), without_memory).expect("admission");
    assert!(
        admission
            .sample()
            .validate_for(
                &profile,
                &plan,
                &"e".repeat(32),
                "test-engine",
                &"f".repeat(64),
                2048,
                Instant::now(),
            )
            .is_err()
    );
    let sample = sample(&profile, true, Instant::now());
    assert!(
        sample
            .validate_for(
                &profile,
                &plan,
                &"e".repeat(32),
                "test-engine",
                &"f".repeat(64),
                2048,
                Instant::now(),
            )
            .is_ok()
    );
}
