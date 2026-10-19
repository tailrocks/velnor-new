use crate::error::HostError;
use crate::journal::connection::ConnectionJournal;
use crate::journal::connection::storage::in_transaction;
use crate::journal::{
    AssignmentKey, Journal, ReservationRequest, ReservationResult, StorageFilesystemKind,
    StorageInodePolicy, StorageMedium, StorageRole, fixture_storage_provisioning_admission,
};
use crate::launch::StartPermit;

use super::admission::JOB_LIMIT;
use super::helpers::{Scratch, binding, created_scope};

const ENGINE_ID: &str = "11111111-1111-4111-8111-111111111111";
const JOB_MEMORY_BYTES: u64 = 8_589_934_592;

#[tokio::test]
async fn volume_claim_exposes_only_role_charge_matching_immutable_profile() -> Result<(), HostError>
{
    let scratch = Scratch::new()?;
    let journal = Journal::open(&scratch.file()).await?;
    journal.configure_capacity(JOB_LIMIT).await?;
    let scope = created_scope(&journal, &binding("role-claims", 701)?, 1701).await?;
    let nonce = "b".repeat(32);
    let assignment = scope
        .assignment_identity(AssignmentKey::Remote {
            kind: "runner".to_owned(),
            subject: nonce,
        })
        .await?;
    let sample_generation_id = "a".repeat(32);
    let permit = StartPermit::test_fixture_with_engine_id(&sample_generation_id, ENGINE_ID);
    let docker_root_digest = permit.docker_root_digest().to_owned();
    let admission = fixture_storage_provisioning_admission(&permit)?;
    let reservation = scope
        .try_reserve_with_provisioning(
            ReservationRequest {
                assignment: &assignment,
                engine_id: ENGINE_ID,
                docker_root_digest: &docker_root_digest,
                cpu_millicores: 2_000,
                memory_bytes: JOB_MEMORY_BYTES,
                sample_generation_id: &sample_generation_id,
            },
            permit,
            admission,
        )
        .await?;
    let ReservationResult::Reserved { reservation_id } = reservation else {
        return Err(HostError::Journal);
    };
    let (_, fresh) = scope.begin_launch(&assignment).await?;
    if fresh {
        return Err(HostError::Journal);
    }

    let profile = scope
        .current_storage_profile()
        .await?
        .ok_or(HostError::Journal)?;
    let role_limit = profile.role_limit(StorageRole::RunnerHome)?;
    let expected_pool = role_limit.pool_id().to_owned();
    let other_pool = if expected_pool == "f".repeat(64) {
        "e".repeat(64)
    } else {
        "f".repeat(64)
    };
    rewrite_role_pool(&scope, reservation_id, &other_pool).await?;
    assert!(matches!(
        scope
            .claim_storage_object_create(reservation_id, StorageRole::RunnerHome)
            .await,
        Err(HostError::Journal)
    ));

    rewrite_role_pool(&scope, reservation_id, &expected_pool).await?;
    let claim = scope
        .claim_storage_object_create(reservation_id, StorageRole::RunnerHome)
        .await?
        .ok_or(HostError::Journal)?;
    let pool = profile.pool_limit(role_limit.pool_id())?;
    assert_eq!(claim.profile_generation_id(), profile.generation_id());
    assert_eq!(claim.profile_digest(), profile.profile_digest());
    assert_eq!(claim.sample_generation_id(), sample_generation_id);
    assert_eq!(claim.pool_id(), role_limit.pool_id());
    assert_eq!(claim.medium(), StorageMedium::Tmpfs);
    assert_eq!(claim.filesystem_kind(), StorageFilesystemKind::Tmpfs);
    assert_eq!(claim.inode_policy(), StorageInodePolicy::Finite);
    assert_eq!(claim.charge_bytes(), role_limit.max_bytes());
    assert_eq!(claim.charge_inodes(), role_limit.max_inodes());
    assert_eq!(
        claim.retained_memory_bytes(),
        role_limit.retained_memory_bytes()
    );
    assert_eq!(pool.inode_policy(), StorageInodePolicy::Finite);

    let invalid_digest = if profile.profile_digest() == "f".repeat(64) {
        "e".repeat(64)
    } else {
        "f".repeat(64)
    };
    rewrite_profile_digest(&scope, reservation_id, &invalid_digest).await?;
    assert!(matches!(
        scope
            .claim_storage_object_create(reservation_id, StorageRole::RunnerWork)
            .await,
        Err(HostError::Journal)
    ));
    Ok(())
}

#[tokio::test]
async fn bootstrap_claim_carries_the_complete_persisted_storage_plan() -> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let journal = Journal::open(&scratch.file()).await?;
    journal.configure_capacity(JOB_LIMIT).await?;
    let scope = created_scope(&journal, &binding("storage-plan", 702)?, 1702).await?;
    let provisioned = super::helpers::provision_physical_intent(&scope, 'c').await?;
    let plan = &provisioned.storage_plan;
    let profile = scope
        .current_storage_profile()
        .await?
        .ok_or(HostError::Journal)?;

    assert_eq!(plan.reservation_id(), provisioned.reservation_id);
    assert_eq!(plan.operation_id(), provisioned.intent_id);
    assert_eq!(plan.scale_set_id(), 1702);
    assert_eq!(plan.engine_id(), ENGINE_ID);
    assert_eq!(plan.docker_root_digest(), provisioned.docker_root_digest);
    assert_eq!(plan.profile_generation_id(), profile.generation_id());
    assert_eq!(plan.profile_digest(), profile.profile_digest());
    assert_eq!(
        plan.sample_generation_id(),
        provisioned.sample_generation_id
    );
    assert_eq!(plan.roles().len(), StorageRole::PHYSICAL.len());

    for role in StorageRole::PHYSICAL {
        let planned = plan
            .roles()
            .iter()
            .find(|planned| planned.role() == role)
            .ok_or(HostError::Journal)?;
        let limit = profile.role_limit(role)?;
        let pool = profile.pool_limit(limit.pool_id())?;
        assert_eq!(planned.pool_id(), limit.pool_id());
        assert_eq!(planned.medium(), limit.medium());
        assert_eq!(
            planned.filesystem_kind(),
            match limit.medium() {
                StorageMedium::Disk => crate::journal::StorageFilesystemKind::Btrfs,
                StorageMedium::Tmpfs => StorageFilesystemKind::Tmpfs,
            }
        );
        assert_eq!(planned.inode_policy(), pool.inode_policy());
        assert_eq!(planned.charge_bytes(), limit.max_bytes());
        assert_eq!(planned.charge_inodes(), limit.max_inodes());
        assert_eq!(
            planned.retained_memory_bytes(),
            limit.retained_memory_bytes()
        );
    }
    Ok(())
}

async fn rewrite_role_pool(
    scope: &ConnectionJournal,
    reservation_id: i64,
    pool_id: &str,
) -> Result<(), HostError> {
    in_transaction(scope.journal_ref(), async |connection| {
        connection
            .execute("DROP TRIGGER provisioning_role_no_update", ())
            .await
            .map_err(|_| HostError::Journal)?;
        let changed = connection
            .execute(
                "UPDATE capacity_reservation_provisioning_roles SET pool_id = ?1 WHERE reservation_id = ?2 AND role = 'runner-home'",
                (pool_id, reservation_id),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        connection
            .execute(
                "CREATE TRIGGER provisioning_role_no_update BEFORE UPDATE ON capacity_reservation_provisioning_roles BEGIN SELECT RAISE(ABORT, 'immutable provisioning role'); END",
                (),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        if changed == 1 {
            Ok(())
        } else {
            Err(HostError::Journal)
        }
    })
    .await
}

async fn rewrite_profile_digest(
    scope: &ConnectionJournal,
    reservation_id: i64,
    digest: &str,
) -> Result<(), HostError> {
    in_transaction(scope.journal_ref(), async |connection| {
        connection
            .execute("DROP TRIGGER provisioning_hold_no_update", ())
            .await
            .map_err(|_| HostError::Journal)?;
        let changed = connection
            .execute(
                "UPDATE capacity_reservation_provisioning SET profile_digest = ?1 WHERE reservation_id = ?2",
                (digest, reservation_id),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        connection
            .execute(
                "CREATE TRIGGER provisioning_hold_no_update BEFORE UPDATE ON capacity_reservation_provisioning BEGIN SELECT RAISE(ABORT, 'immutable provisioning hold'); END",
                (),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        if changed == 1 {
            Ok(())
        } else {
            Err(HostError::Journal)
        }
    })
    .await
}
