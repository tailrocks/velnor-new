use crate::error::HostError;
use crate::journal::request_lifecycle::tests::helpers::{Scratch, binding, created_scope};
use crate::journal::{
    AssignmentKey, CapacityLimits, Journal, OperationalCapacityLimits, ReservationRequest,
    ReservationResult, StorageAdmissionSample, StorageFilesystemKind, StorageMedium,
    StoragePoolObservation, StoragePoolObservationInput, StorageProfile,
    StorageProvisioningAdmission, StorageProvisioningMemoryObservation, StorageSampleIdentity,
    fixture_profile, fixture_provisioning_profile,
};
use crate::launch::StartPermit;

const ENGINE_ID: &str = "11111111-1111-4111-8111-111111111111";
const JOB_MEMORY: u64 = 8 * 1024 * 1024 * 1024;
const GIB: u64 = 1024 * 1024 * 1024;

fn permit(generation: &str, cpu: u64, total: u64, available: u64) -> StartPermit {
    StartPermit::test_fixture_with_guest_capacity(generation, ENGINE_ID, cpu, total, available)
}

fn admission(
    permit: &StartPermit,
    profile: &StorageProfile,
    total_memory: u64,
    available_memory: u64,
) -> Result<StorageProvisioningAdmission, HostError> {
    let disk = profile
        .pools()
        .iter()
        .find(|pool| pool.medium() == StorageMedium::Disk)
        .ok_or(HostError::Journal)?;
    let sample = StorageAdmissionSample::new(
        StorageSampleIdentity::new(
            permit.sample_generation_id().to_owned(),
            profile.generation_id().to_owned(),
            profile.profile_digest().to_owned(),
            ENGINE_ID.to_owned(),
            permit.docker_root_digest().to_owned(),
        )?,
        std::time::Instant::now(),
        vec![StoragePoolObservation::new_with_filesystem(
            StoragePoolObservationInput {
                pool_id: disk.pool_id().to_owned(),
                filesystem_instance_digest: "a".repeat(64),
                medium: StorageMedium::Disk,
                filesystem_kind: StorageFilesystemKind::Btrfs,
                total_bytes: 64 * GIB,
                available_bytes: 60 * GIB,
                total_inodes: 0,
                available_inodes: 0,
            },
        )?],
        None,
    )?;
    Ok(StorageProvisioningAdmission::new(
        sample,
        StorageProvisioningMemoryObservation::new(total_memory, available_memory)?,
    ))
}

async fn reserve(
    scope: &crate::journal::ConnectionJournal,
    profile: &StorageProfile,
    generation: char,
    cpu: u64,
    total_memory: u64,
    available_memory: u64,
) -> Result<ReservationResult, HostError> {
    let sample_id = generation.to_string().repeat(32);
    reserve_for_assignment(
        scope,
        profile,
        generation,
        &sample_id,
        cpu,
        total_memory,
        available_memory,
    )
    .await
}

async fn reserve_for_assignment(
    scope: &crate::journal::ConnectionJournal,
    profile: &StorageProfile,
    generation: char,
    assignment_subject: &str,
    cpu: u64,
    total_memory: u64,
    available_memory: u64,
) -> Result<ReservationResult, HostError> {
    let sample_id = generation.to_string().repeat(32);
    let permit = permit(&sample_id, cpu, total_memory, available_memory);
    let root = permit.docker_root_digest().to_owned();
    let assignment = scope
        .assignment_identity(AssignmentKey::Remote {
            kind: "runner".to_owned(),
            subject: assignment_subject.to_owned(),
        })
        .await?;
    let admission = admission(&permit, profile, total_memory, available_memory)?;
    scope
        .try_reserve_with_provisioning(
            ReservationRequest {
                assignment: &assignment,
                engine_id: ENGINE_ID,
                docker_root_digest: &root,
                cpu_millicores: 2_000,
                memory_bytes: JOB_MEMORY,
                sample_generation_id: &sample_id,
            },
            permit,
            admission,
        )
        .await
}

fn retained_memory(profile: &StorageProfile) -> Result<u64, HostError> {
    profile
        .roles()
        .iter()
        .filter(|role| role.alias_of().is_none())
        .try_fold(0_u64, |sum, role| {
            sum.checked_add(role.retained_memory_bytes())
                .ok_or(HostError::Journal)
        })
}

#[tokio::test]
async fn shrunk_guest_rejects_new_admission_and_keeps_existing_charge() -> Result<(), HostError> {
    const REQUIRED_CPU: u64 = 13_000;
    const REQUIRED_MEMORY: u64 = 25 * GIB;
    let scratch = Scratch::new()?;
    let journal = Journal::open(&scratch.file()).await?;
    journal
        .configure_capacity_with_operational_limits(
            CapacityLimits {
                jobs: 3,
                cpu_millicores: 12_000,
                memory_bytes: 24 * GIB,
            },
            OperationalCapacityLimits {
                cpu_millicores: 1_000,
                memory_bytes: GIB,
            },
        )
        .await?;
    let binding = binding("essential", 101)?;
    let scope = created_scope(&journal, &binding, 501).await?;
    let profile = fixture_provisioning_profile();
    journal.configure_storage_profile(profile.clone()).await?;

    assert!(matches!(
        reserve(
            &scope,
            &profile,
            'e',
            REQUIRED_CPU,
            REQUIRED_MEMORY,
            REQUIRED_MEMORY
        )
        .await?,
        ReservationResult::Reserved { .. }
    ));
    assert_eq!(
        reserve(
            &scope,
            &profile,
            'f',
            REQUIRED_CPU - 1,
            REQUIRED_MEMORY,
            REQUIRED_MEMORY
        )
        .await?,
        ReservationResult::AtCapacity
    );
    assert_eq!(
        reserve(
            &scope,
            &profile,
            'a',
            REQUIRED_CPU,
            REQUIRED_MEMORY - 1,
            REQUIRED_MEMORY - 1
        )
        .await?,
        ReservationResult::AtCapacity
    );
    assert_eq!(
        reserve(
            &scope,
            &profile,
            'b',
            REQUIRED_CPU,
            REQUIRED_MEMORY,
            19 * GIB
        )
        .await?,
        ReservationResult::AtCapacity
    );
    assert_eq!(journal.occupied_reservation_count().await?, 1);
    Ok(())
}

#[tokio::test]
async fn generic_reservation_path_rejects_shrunk_aggregate_capacity() -> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let journal = Journal::open(&scratch.file()).await?;
    journal
        .configure_capacity_with_operational_limits(
            CapacityLimits {
                jobs: 3,
                cpu_millicores: 12_000,
                memory_bytes: 24 * GIB,
            },
            OperationalCapacityLimits {
                cpu_millicores: 1_000,
                memory_bytes: GIB,
            },
        )
        .await?;
    let binding = binding("essential", 101)?;
    let scope = created_scope(&journal, &binding, 501).await?;
    journal.configure_storage_profile(fixture_profile()).await?;

    for (nonce, cpu, total, expect_capacity) in [
        ('e', 13_000, 25 * GIB, false),
        ('f', 12_999, 25 * GIB, true),
        ('a', 13_000, 25 * GIB - 1, true),
    ] {
        let sample_id = nonce.to_string().repeat(32);
        let permit = permit(&sample_id, cpu, total, total);
        let root = permit.docker_root_digest().to_owned();
        let assignment = scope
            .assignment_identity(AssignmentKey::Remote {
                kind: "runner".to_owned(),
                subject: sample_id.clone(),
            })
            .await?;
        let storage = crate::journal::fixture_admission(&permit)?;
        let result = scope
            .try_reserve_with_storage(
                ReservationRequest {
                    assignment: &assignment,
                    engine_id: ENGINE_ID,
                    docker_root_digest: &root,
                    cpu_millicores: 2_000,
                    memory_bytes: JOB_MEMORY,
                    sample_generation_id: &sample_id,
                },
                permit,
                storage,
            )
            .await?;
        if expect_capacity {
            assert_eq!(result, ReservationResult::AtCapacity);
        } else {
            assert!(matches!(result, ReservationResult::Reserved { .. }));
        }
    }
    assert_eq!(journal.occupied_reservation_count().await?, 1);
    Ok(())
}

#[tokio::test]
async fn held_jobs_reduce_available_memory_across_scopes_and_resume_excludes_own_charge()
-> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let journal = Journal::open(&scratch.file()).await?;
    journal
        .configure_capacity_with_operational_limits(
            CapacityLimits {
                jobs: 3,
                cpu_millicores: 60_000,
                memory_bytes: 60 * GIB,
            },
            OperationalCapacityLimits {
                cpu_millicores: 1_000,
                memory_bytes: GIB,
            },
        )
        .await?;
    let profile = fixture_provisioning_profile();
    journal.configure_storage_profile(profile.clone()).await?;

    let first_binding = binding("essential", 101)?;
    let first_scope = created_scope(&journal, &first_binding, 501).await?;
    let second_binding = binding("essential-secondary", 102)?;
    let second_scope = created_scope(&journal, &second_binding, 502).await?;

    let reservation_id =
        match reserve(&first_scope, &profile, 'e', 61_000, 64 * GIB, 25 * GIB).await? {
            ReservationResult::Reserved { reservation_id } => reservation_id,
            result => panic!(
                "The capacity check did not accept the first reservation. Result: {result:?}"
            ),
        };
    assert_eq!(
        reserve(&second_scope, &profile, 'f', 61_000, 64 * GIB, 25 * GIB).await?,
        ReservationResult::AtCapacity
    );
    assert_eq!(journal.occupied_reservation_count().await?, 1);

    let exact_available = JOB_MEMORY
        .checked_add(retained_memory(&profile)?)
        .and_then(|bytes| bytes.checked_add(GIB))
        .ok_or(HostError::Journal)?;
    let sample_id = 'a'.to_string().repeat(32);
    let resume_permit = permit(&sample_id, 61_000, 64 * GIB, exact_available);
    let resume_capacity = crate::journal::connection::storage::in_transaction(
        first_scope.journal_ref(),
        async |connection| {
            crate::journal::connection::reservation::storage::guest_capacity_fits(
                connection,
                &resume_permit,
                JOB_MEMORY,
                retained_memory(&profile)?,
                Some(reservation_id),
            )
            .await
        },
    )
    .await?;
    let double_counted_capacity = crate::journal::connection::storage::in_transaction(
        first_scope.journal_ref(),
        async |connection| {
            crate::journal::connection::reservation::storage::guest_capacity_fits(
                connection,
                &resume_permit,
                JOB_MEMORY,
                retained_memory(&profile)?,
                None,
            )
            .await
        },
    )
    .await?;
    assert!(resume_capacity);
    assert!(!double_counted_capacity);
    assert_eq!(journal.occupied_reservation_count().await?, 1);
    Ok(())
}
