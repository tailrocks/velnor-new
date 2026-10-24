use super::helpers::{Scratch, binding, created_scope, receipt};
use crate::error::HostError;
use crate::journal::{
    AssignedAcquireResult, AssignmentKey, CapacityLimits, Journal, RequestAcquireState,
    RequestPhysicalReservationResult, ReservationRequest, SourceAuthorizationResult,
    fixture_storage_provisioning_admission,
};
use crate::launch::StartPermit;

const ENGINE_ID: &str = super::helpers::TEST_ENGINE_ID;
const JOB_MEMORY: u64 = 8_589_934_592;

async fn configured_scope(
    path: &std::path::Path,
) -> Result<(Journal, crate::journal::ConnectionJournal), HostError> {
    let journal = Journal::open(path).await?;
    journal
        .configure_capacity(CapacityLimits {
            jobs: 1,
            cpu_millicores: 2_000,
            memory_bytes: 25_769_803_776,
        })
        .await?;
    let scope = created_scope(&journal, &binding("essential", 101)?, 501).await?;
    Ok((journal, scope))
}

async fn reserve_and_claim(
    scope: &crate::journal::ConnectionJournal,
    request_id: i64,
    nonce: char,
    sample: char,
) -> Result<RequestPhysicalReservationResult, HostError> {
    let generation = sample.to_string().repeat(32);
    let permit = StartPermit::test_fixture_with_engine_id(&generation, ENGINE_ID);
    let docker_root_digest = permit.docker_root_digest().to_owned();
    let admission = fixture_storage_provisioning_admission(&permit)?;
    let physical = scope
        .assignment_identity(AssignmentKey::Remote {
            kind: "runner".to_owned(),
            subject: nonce.to_string().repeat(32),
        })
        .await?;
    let request = scope
        .assignment_identity(AssignmentKey::AssignedRequest { request_id })
        .await?;
    scope
        .try_reserve_and_claim_request_acquire(
            ReservationRequest {
                assignment: &physical,
                engine_id: ENGINE_ID,
                docker_root_digest: &docker_root_digest,
                cpu_millicores: 2_000,
                memory_bytes: JOB_MEMORY,
                sample_generation_id: &generation,
            },
            permit,
            admission,
            &request,
        )
        .await
}

#[tokio::test]
async fn physical_hold_and_acquire_claim_commit_together_and_replay_same_link()
-> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let path = scratch.file();
    let (journal, scope) = configured_scope(&path).await?;
    let request = scope
        .assignment_identity(AssignmentKey::AssignedRequest { request_id: 811 })
        .await?;
    let source = receipt(&scope, 811, 911, 101).await?;
    assert_eq!(
        scope.record_source_authorization(&request, &source).await?,
        SourceAuthorizationResult::Recorded
    );

    let first = reserve_and_claim(&scope, 811, '1', '2').await?;
    let RequestPhysicalReservationResult::AcquireClaimed {
        reservation_id,
        intent_id,
    } = first
    else {
        return Err(HostError::Journal);
    };
    assert!(reservation_id > 0 && intent_id > 0);
    assert_eq!(
        scope.request_acquire_state(&request).await?,
        Some(RequestAcquireState::Requested)
    );
    assert!(!scope.claim_request_acquire(&request).await?);

    let replay = reserve_and_claim(&scope, 811, '1', '3').await?;
    assert!(matches!(
        replay,
        RequestPhysicalReservationResult::Existing {
            reservation_id: same_reservation,
            intent_id: same_intent,
            acquire_state: RequestAcquireState::Requested,
        } if same_reservation == reservation_id && same_intent == intent_id
    ));
    scope
        .record_request_acquire(&request, AssignedAcquireResult::Returned(vec![811]))
        .await?;
    assert_eq!(
        scope.request_acquire_state(&request).await?,
        Some(RequestAcquireState::Acquired)
    );
    drop(scope);
    drop(journal);

    let journal = Journal::open(&path).await?;
    let scope = journal
        .connection_scope(&binding("essential", 101)?)
        .await?;
    let request = scope
        .assignment_identity(AssignmentKey::AssignedRequest { request_id: 811 })
        .await?;
    assert_eq!(
        scope.request_acquire_state(&request).await?,
        Some(RequestAcquireState::Acquired)
    );
    let replay = reserve_and_claim(&scope, 811, '1', '4').await?;
    assert!(matches!(
        replay,
        RequestPhysicalReservationResult::Existing {
            reservation_id: same_reservation,
            intent_id: same_intent,
            acquire_state: RequestAcquireState::Acquired,
        } if same_reservation == reservation_id && same_intent == intent_id
    ));
    let physical = scope
        .assignment_identity(AssignmentKey::Remote {
            kind: "runner".to_owned(),
            subject: "1".repeat(32),
        })
        .await?;
    assert_eq!(
        scope
            .reservation(&physical)
            .await?
            .map(|row| row.reservation_id),
        Some(reservation_id)
    );
    Ok(())
}

#[tokio::test]
async fn missing_authorization_rolls_back_physical_hold_and_request_claim() -> Result<(), HostError>
{
    let scratch = Scratch::new()?;
    let (journal, scope) = configured_scope(&scratch.file()).await?;
    assert!(reserve_and_claim(&scope, 812, '5', '6').await.is_err());
    let physical = scope
        .assignment_identity(AssignmentKey::Remote {
            kind: "runner".to_owned(),
            subject: "5".repeat(32),
        })
        .await?;
    assert!(scope.reservation(&physical).await?.is_none());
    assert_eq!(
        scope
            .request_acquire_state(
                &scope
                    .assignment_identity(AssignmentKey::AssignedRequest { request_id: 812 })
                    .await?,
            )
            .await?,
        None
    );
    assert!(journal.is_drained().await?);
    Ok(())
}

#[tokio::test]
async fn request_link_rejects_wrong_nonce_kind_and_scale_set() -> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let (journal, scope) = configured_scope(&scratch.file()).await?;
    let request = scope
        .assignment_identity(AssignmentKey::AssignedRequest { request_id: 813 })
        .await?;
    scope
        .record_source_authorization(&request, &receipt(&scope, 813, 913, 101).await?)
        .await?;
    let generation = "7".repeat(32);
    let permit = StartPermit::test_fixture_with_engine_id(&generation, ENGINE_ID);
    let docker_root_digest = permit.docker_root_digest().to_owned();
    let admission = fixture_storage_provisioning_admission(&permit)?;
    let bad_physical = scope
        .assignment_identity(AssignmentKey::Remote {
            kind: "other".to_owned(),
            subject: "8".repeat(32),
        })
        .await?;
    assert!(
        scope
            .try_reserve_and_claim_request_acquire(
                ReservationRequest {
                    assignment: &bad_physical,
                    engine_id: ENGINE_ID,
                    docker_root_digest: &docker_root_digest,
                    cpu_millicores: 2_000,
                    memory_bytes: JOB_MEMORY,
                    sample_generation_id: &generation,
                },
                permit,
                admission,
                &request,
            )
            .await
            .is_err()
    );
    let physical = scope
        .assignment_identity(AssignmentKey::Remote {
            kind: "other".to_owned(),
            subject: "8".repeat(32),
        })
        .await?;
    assert!(scope.reservation(&physical).await?.is_none());

    let other_scope = created_scope(&journal, &binding("chain", 202)?, 502).await?;
    let other_generation = "b".repeat(32);
    let other_permit = StartPermit::test_fixture_with_engine_id(&other_generation, ENGINE_ID);
    let other_root_digest = other_permit.docker_root_digest().to_owned();
    let other_admission = fixture_storage_provisioning_admission(&other_permit)?;
    let other_physical = other_scope
        .assignment_identity(AssignmentKey::Remote {
            kind: "runner".to_owned(),
            subject: "9".repeat(32),
        })
        .await?;
    assert!(
        other_scope
            .try_reserve_and_claim_request_acquire(
                ReservationRequest {
                    assignment: &other_physical,
                    engine_id: ENGINE_ID,
                    docker_root_digest: &other_root_digest,
                    cpu_millicores: 2_000,
                    memory_bytes: JOB_MEMORY,
                    sample_generation_id: &other_generation,
                },
                other_permit,
                other_admission,
                &request,
            )
            .await
            .is_err()
    );
    assert!(other_scope.reservation(&other_physical).await?.is_none());
    Ok(())
}
