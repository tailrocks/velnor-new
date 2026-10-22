use crate::error::HostError;
use crate::journal::request_lifecycle::tests::helpers::{
    Scratch, TEST_ENGINE_ID, binding, created_scope,
};
use crate::journal::{
    AssignmentKey, CapacityLimits, Journal, ReservationRequest, ReservationResult,
    fixture_storage_provisioning_admission,
};
use crate::launch::StartPermit;

#[tokio::test]
async fn private_network_intent_survives_reopen_and_never_replays_implicitly()
-> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let path = scratch.file();
    let connection = binding("private-network", 7_001)?;
    let journal = Journal::open(&path).await?;
    journal
        .configure_capacity(CapacityLimits {
            jobs: 1,
            cpu_millicores: 2_000,
            memory_bytes: 32 * 1024 * 1024 * 1024,
        })
        .await?;
    let scope = created_scope(&journal, &connection, 7_501).await?;
    let reservation_id = reserve_private_network(&scope).await?;

    let claim = scope
        .claim_storage_network_create(reservation_id)
        .await?
        .ok_or(HostError::Journal)?;
    assert_eq!(claim.reservation_id(), reservation_id);
    assert_eq!(claim.engine_id(), TEST_ENGINE_ID);
    assert!(claim.network_name().starts_with("velnor-v"));
    assert_eq!(claim.network_name().len(), 48);
    assert_eq!(
        claim.labels().get("io.velnor.managed").map(String::as_str),
        Some("true")
    );
    assert!(
        scope
            .claim_storage_network_create(reservation_id)
            .await?
            .is_none()
    );
    scope.mark_storage_network_create_uncertain(&claim).await?;

    drop(claim);
    drop(scope);
    drop(journal);

    let reopened = Journal::open(&path).await?;
    let scope = reopened.connection_scope(&connection).await?;
    let identity = scope
        .storage_network_recovery_identity(reservation_id)
        .await?
        .ok_or(HostError::Journal)?;
    assert_eq!(identity.create_state(), "uncertain");
    assert_eq!(identity.remove_state(), "not_requested");
    assert_eq!(identity.engine_id(), TEST_ENGINE_ID);
    assert_eq!(
        identity.network_name(),
        "velnor-va1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1-private"
    );
    let recovery_claim = scope
        .recover_storage_network_create(reservation_id)
        .await?
        .ok_or(HostError::Journal)?;
    assert_eq!(recovery_claim.network_name(), identity.network_name());
    assert!(
        scope
            .claim_storage_network_create(reservation_id)
            .await?
            .is_none()
    );
    Ok(())
}

async fn reserve_private_network(
    scope: &crate::journal::ConnectionJournal,
) -> Result<i64, HostError> {
    const SAMPLE: &str = "a1";
    let subject = SAMPLE.repeat(16);
    let assignment = scope
        .assignment_identity(AssignmentKey::Remote {
            kind: "runner".to_owned(),
            subject,
        })
        .await?;
    let permit = StartPermit::test_fixture_with_engine_id(&SAMPLE.repeat(16), TEST_ENGINE_ID);
    let sample_generation_id = permit.sample_generation_id().to_owned();
    let docker_root_digest = permit.docker_root_digest().to_owned();
    let admission = fixture_storage_provisioning_admission(&permit)?;
    let result = scope
        .try_reserve_with_provisioning(
            ReservationRequest {
                assignment: &assignment,
                engine_id: TEST_ENGINE_ID,
                docker_root_digest: &docker_root_digest,
                cpu_millicores: 2_000,
                memory_bytes: 8 * 1024 * 1024 * 1024,
                sample_generation_id: &sample_generation_id,
            },
            permit,
            admission,
        )
        .await?;
    let ReservationResult::Reserved { reservation_id } = result else {
        return Err(HostError::Journal);
    };
    Ok(reservation_id)
}
