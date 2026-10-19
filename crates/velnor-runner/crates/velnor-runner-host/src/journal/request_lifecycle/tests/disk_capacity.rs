use super::helpers::{Scratch, binding, created_scope};
use crate::error::HostError;
use crate::journal::{
    AssignmentKey, CapacityLimits, Journal, ReservationRequest, ReservationResult,
    fixture_storage_provisioning_admission,
};
use crate::launch::StartPermit;

const ENGINE_ID: &str = super::helpers::TEST_ENGINE_ID;
const JOB_MEMORY: u64 = 8_589_934_592;

#[tokio::test]
async fn shared_disk_profile_limit_charges_all_connection_reservations() -> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let journal = Journal::open(&scratch.file()).await?;
    journal
        .configure_capacity(CapacityLimits {
            jobs: 8,
            cpu_millicores: 16_000,
            memory_bytes: 128 * 1024 * 1024 * 1024,
        })
        .await?;
    let first_binding = binding("essential", 101)?;
    let second_binding = binding("chain", 202)?;
    let first = created_scope(&journal, &first_binding, 501).await?;
    let second = created_scope(&journal, &second_binding, 502).await?;

    let (first_reserved, Some(_first_id), first_assignment) = reserve(&first, 'a').await? else {
        return Err(HostError::Journal);
    };
    assert!(matches!(first_reserved, ReservationResult::Reserved { .. }));
    let (second_reserved, Some(second_id), second_assignment) = reserve(&second, 'b').await? else {
        return Err(HostError::Journal);
    };
    assert!(matches!(
        second_reserved,
        ReservationResult::Reserved { .. }
    ));
    second.mark_uncertain(second_id).await?;
    let (third_reserved, Some(third_id), third_assignment) = reserve(&first, 'c').await? else {
        return Err(HostError::Journal);
    };
    assert!(matches!(third_reserved, ReservationResult::Reserved { .. }));
    first.mark_recovering(third_id).await?;

    let (fourth, None, _) = reserve(&second, 'd').await? else {
        return Err(HostError::Journal);
    };
    assert_eq!(fourth, ReservationResult::AtCapacity);
    assert_eq!(
        first
            .reservation(&first_assignment)
            .await?
            .map(|row| row.state),
        Some(crate::journal::ReservationState::Reserved)
    );
    assert_eq!(
        second
            .reservation(&second_assignment)
            .await?
            .map(|row| row.state),
        Some(crate::journal::ReservationState::Uncertain)
    );
    assert_eq!(
        first
            .reservation(&third_assignment)
            .await?
            .map(|row| row.state),
        Some(crate::journal::ReservationState::Recovering)
    );
    Ok(())
}

#[tokio::test]
async fn metadata_settlement_holds_when_active_charge_exceeds_profile_cap() -> Result<(), HostError>
{
    let scratch = Scratch::new()?;
    let journal = Journal::open(&scratch.file()).await?;
    journal
        .configure_capacity(CapacityLimits {
            jobs: 4,
            cpu_millicores: 8_000,
            memory_bytes: 64 * 1024 * 1024 * 1024,
        })
        .await?;
    let binding = binding("essential", 101)?;
    let scope = created_scope(&journal, &binding, 501).await?;
    let target = super::helpers::provision_physical_intent(&scope, '1').await?;
    let other = super::helpers::provision_physical_intent(&scope, '2').await?;

    crate::journal::connection::storage::in_transaction(scope.journal_ref(), async |connection| {
        connection
            .execute("DROP TRIGGER provisioning_role_no_update", ())
            .await
            .map_err(|_| HostError::Journal)?;
        let changed = connection
            .execute(
                "UPDATE capacity_reservation_provisioning_roles SET charge_bytes = ?1 WHERE rowid = (SELECT rowid FROM capacity_reservation_provisioning_roles WHERE reservation_id = ?2 AND pool_id = ?3 LIMIT 1)",
                (20_i64 * 1024 * 1024 * 1024, other.reservation_id, "1".repeat(64)),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        if changed != 1 {
            return Err(HostError::Journal);
        }
        connection
            .execute(
                "CREATE TRIGGER provisioning_role_no_update BEFORE UPDATE ON capacity_reservation_provisioning_roles BEGIN SELECT RAISE(ABORT, 'immutable provisioning role'); END",
                (),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        Ok(())
    })
    .await?;

    super::helpers::record_owned_storage_cleanup(&scope, &target).await?;
    scope.record_remote_cleanup(target.reservation_id).await?;
    scope.record_local_cleanup(target.reservation_id).await?;
    let settlement = super::helpers::settle_storage_metadata(&scope, &target, '3').await?;
    assert_eq!(
        settlement,
        crate::journal::MetadataSettlementResult::HeldAtCapacity
    );
    Ok(())
}

async fn reserve(
    scope: &crate::journal::ConnectionJournal,
    nonce: char,
) -> Result<
    (
        ReservationResult,
        Option<i64>,
        crate::journal::AssignmentIdentity,
    ),
    HostError,
> {
    let sample = nonce.to_string().repeat(32);
    let assignment = scope
        .assignment_identity(AssignmentKey::Remote {
            kind: "runner".to_owned(),
            subject: sample.clone(),
        })
        .await?;
    let permit = StartPermit::test_fixture_with_engine_id(&sample, ENGINE_ID);
    let sample_generation_id = permit.sample_generation_id().to_owned();
    let docker_root_digest = permit.docker_root_digest().to_owned();
    let admission = fixture_storage_provisioning_admission(&permit)?;
    let result = scope
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
        .await?;
    let id = match result {
        ReservationResult::Reserved { reservation_id } => Some(reservation_id),
        ReservationResult::AtCapacity => None,
        _ => return Err(HostError::Journal),
    };
    Ok((result, id, assignment))
}
