use super::*;

pub(super) fn profile(medium: StorageMedium) -> StorageProfile {
    StorageProfile::new(
        "a".repeat(32),
        vec![
            StoragePoolLimit::new("c".repeat(64), medium, 16_384, 256, 128, 4).expect("pool limit"),
        ],
        role_limits(medium, 12_000),
    )
    .expect("profile")
}

pub(super) fn role_limits(medium: StorageMedium, work_bytes: u64) -> Vec<StorageRoleLimit> {
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

pub(super) fn role(medium: StorageMedium) -> StorageRoleCharge {
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

pub(super) fn sample(
    profile: &StorageProfile,
    memory: bool,
    observed_at: Instant,
) -> StorageAdmissionSample {
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

pub(super) fn plan(profile: &StorageProfile, medium: StorageMedium) -> StorageReservationPlan {
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
