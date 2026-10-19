use super::{charge_fits_pool, checked_required_charge};
use crate::error::HostError;

#[test]
fn aggregate_charge_must_fit_profile_and_observed_capacity() {
    let second_reservation = checked_required_charge(4, 4, 2).expect("checked aggregate");
    assert_eq!(second_reservation, 10);
    assert!(!charge_fits_pool(second_reservation, 8, 60));
    assert!(charge_fits_pool(second_reservation, 10, 60));
    assert!(!charge_fits_pool(second_reservation, 10, 9));
}

#[test]
fn aggregate_charge_overflow_fails_closed() {
    assert!(matches!(
        checked_required_charge(u64::MAX, 1, 0),
        Err(HostError::Journal)
    ));
    assert!(matches!(
        checked_required_charge(1, 1, u64::MAX),
        Err(HostError::Journal)
    ));
}

#[tokio::test]
async fn finite_inode_pool_cap_rejects_second_reservation_with_free_inodes() -> Result<(), HostError>
{
    use crate::journal::request_lifecycle::tests::helpers::{Scratch, binding, created_scope};
    use crate::journal::{
        AssignmentKey, CapacityLimits, Journal, ReservationRequest, ReservationResult,
        StorageAdmissionSample, StorageFilesystemKind, StorageInodePolicy, StorageMedium,
        StoragePoolObservation, StorageProfile, StorageSampleIdentity,
        fixture_provisioning_profile,
    };
    use crate::launch::StartPermit;

    const ENGINE_ID: &str = "11111111-1111-4111-8111-111111111111";
    const JOB_MEMORY: u64 = 8_589_934_592;
    let scratch = Scratch::new()?;
    let journal = Journal::open(&scratch.file()).await?;
    journal
        .configure_capacity(CapacityLimits {
            jobs: 8,
            cpu_millicores: 16_000,
            memory_bytes: 128 * 1024 * 1024 * 1024,
        })
        .await?;
    let binding = binding("essential", 101)?;
    let scope = created_scope(&journal, &binding, 501).await?;
    let fixture = fixture_provisioning_profile();
    let disk_id = fixture
        .pools()
        .iter()
        .find(|pool| pool.medium() == StorageMedium::Disk)
        .ok_or(HostError::Journal)?
        .pool_id()
        .to_owned();
    let pools = fixture
        .pools()
        .iter()
        .map(|pool| {
            if pool.pool_id() == disk_id {
                super::super::super::storage_budget::StoragePoolLimit::new_with_inode_policy(
                    disk_id.clone(),
                    StorageMedium::Disk,
                    StorageInodePolicy::Finite,
                    pool.max_bytes(),
                    31_000,
                    pool.reserve_bytes(),
                    1_000,
                )
            } else {
                Ok(pool.clone())
            }
        })
        .collect::<Result<Vec<_>, HostError>>()?;
    let profile = StorageProfile::new("d".repeat(32), pools, fixture.roles().to_vec())?;
    journal.configure_storage_profile(profile.clone()).await?;

    let mut outcomes = Vec::new();
    for nonce in ['e', 'f'] {
        let sample_id = nonce.to_string().repeat(32);
        let assignment = scope
            .assignment_identity(AssignmentKey::Remote {
                kind: "runner".to_owned(),
                subject: sample_id.clone(),
            })
            .await?;
        let permit = StartPermit::test_fixture_with_engine_id(&sample_id, ENGINE_ID);
        let sample_generation_id = permit.sample_generation_id().to_owned();
        let docker_root_digest = permit.docker_root_digest().to_owned();
        let sample = StorageAdmissionSample::new(
            StorageSampleIdentity::new(
                sample_generation_id.clone(),
                profile.generation_id().to_owned(),
                profile.profile_digest().to_owned(),
                ENGINE_ID.to_owned(),
                docker_root_digest.clone(),
            )?,
            std::time::Instant::now(),
            vec![StoragePoolObservation::new_with_filesystem(
                disk_id.clone(),
                "a".repeat(64),
                StorageMedium::Disk,
                StorageFilesystemKind::Other,
                64_u64 << 30,
                60_u64 << 30,
                1_000_000,
                1_000_000,
            )?],
            None,
        )?;
        let admission = super::StorageProvisioningAdmission::new(
            sample,
            super::super::super::storage_budget::StorageProvisioningMemoryObservation::new(
                64_u64 << 30,
                32_u64 << 30,
            )?,
        );
        outcomes.push(
            scope
                .try_reserve_with_provisioning(
                    ReservationRequest {
                        assignment: &assignment,
                        engine_id: ENGINE_ID,
                        docker_root_digest: &docker_root_digest,
                        cpu_millicores: 2_000,
                        memory_bytes: JOB_MEMORY,
                        sample_generation_id: &sample_generation_id,
                    },
                    permit,
                    admission,
                )
                .await?,
        );
    }
    assert!(matches!(outcomes[0], ReservationResult::Reserved { .. }));
    assert_eq!(outcomes[1], ReservationResult::AtCapacity);
    Ok(())
}
