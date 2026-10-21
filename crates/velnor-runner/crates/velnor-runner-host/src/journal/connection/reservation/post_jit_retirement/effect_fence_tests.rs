use crate::error::HostError;
use crate::journal::request_lifecycle::tests::native_started_intake_fixture;

const RETIREMENT_ERROR: &str = "A retirement claim prevents this operation.";

#[tokio::test]
async fn retirement_claim_prevents_listener_delivery_request() -> Result<(), HostError> {
    let fixture = native_started_intake_fixture().await?;
    super::test_support::recover_fixture(&fixture).await?;
    fixture
        .scope
        .claim_post_jit_retirement(fixture.reservation_id, 60)
        .await?
        .ok_or(HostError::Journal)?;

    let changed = fixture
        .journal
        .connection()
        .await?
        .execute(
            "UPDATE capacity_reservation_provisioning_state SET delivery_state='requested' WHERE reservation_id=?1 AND delivery_state='delivered'",
            [fixture.reservation_id],
        )
        .await;
    assert_retirement_error(changed);

    let mut rows = fixture
        .journal
        .connection()
        .await?
        .query(
            "SELECT delivery_state FROM capacity_reservation_provisioning_state WHERE reservation_id=?1",
            [fixture.reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let state: String = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get(0)
        .map_err(|_| HostError::Journal)?;
    assert_eq!(state, "delivered");
    Ok(())
}

#[tokio::test]
async fn retirement_claim_prevents_all_provisioning_requests() -> Result<(), HostError> {
    let fixture = native_started_intake_fixture().await?;
    super::test_support::recover_fixture(&fixture).await?;
    let attempt = fixture
        .scope
        .current_physical_attempt(fixture.reservation_id)
        .await?
        .ok_or(HostError::Journal)?;
    fixture
        .scope
        .claim_post_jit_retirement(fixture.reservation_id, 60)
        .await?
        .ok_or(HostError::Journal)?;

    let connection = fixture.journal.connection().await?;
    for (column, value) in [
        ("jit_state", "requested"),
        ("publication_state", "requested"),
        ("delivery_state", "requested"),
    ] {
        let sql = format!(
            "UPDATE capacity_reservation_provisioning_state SET {column}=?1 WHERE reservation_id=?2"
        );
        assert_retirement_error(
            connection
                .execute(&sql, (value, fixture.reservation_id))
                .await,
        );
    }

    let jit_insert = connection
        .execute(
            "INSERT INTO connection_runner_jits (intent_id,connection_id,scale_set_id,witness_request_id,requested_name,state,runner_id,runner_name) VALUES (?1,?2,?3,?4,?5,'claimed',NULL,NULL)",
            (attempt.intent_id, fixture.scope.connection_id().as_str(), fixture.channel.scale_set_id(), fixture.request_id, fixture.runner.name.as_str()),
        )
        .await;
    assert_retirement_error(jit_insert);
    Ok(())
}

#[tokio::test]
async fn retirement_claim_prevents_challenge_decision_and_publication_requests()
-> Result<(), HostError> {
    let fixture = native_started_intake_fixture().await?;
    super::test_support::recover_fixture(&fixture).await?;
    let attempt = fixture
        .scope
        .current_physical_attempt(fixture.reservation_id)
        .await?
        .ok_or(HostError::Journal)?;
    fixture
        .scope
        .claim_post_jit_retirement(fixture.reservation_id, 60)
        .await?
        .ok_or(HostError::Journal)?;

    let connection = fixture.journal.connection().await?;
    assert_retirement_error(
        connection
            .execute(
                "UPDATE connection_execution_proofs SET challenge_state='requested' WHERE intent_id=?1 AND reservation_id=?2 AND connection_id=?3",
                (attempt.intent_id, fixture.reservation_id, fixture.scope.connection_id().as_str()),
            )
            .await,
    );

    assert_retirement_error(
        connection
            .execute(
                "INSERT INTO connection_execution_decisions (intent_id,reservation_id,connection_id,scale_set_id,witness_request_id,actual_request_id,challenge,generation,policy_digest,request_digest,allow,decision_digest,decision_json,delivery_state) VALUES (?1,?2,?3,?4,?5,?5,?6,1,?6,?6,1,?6,'{}','prepared')",
                (attempt.intent_id, fixture.reservation_id, fixture.scope.connection_id().as_str(), fixture.channel.scale_set_id(), fixture.channel.witness_request_id(), "a".repeat(64)),
            )
            .await,
    );

    assert_retirement_error(
        connection
            .execute(
                "INSERT INTO execution_publication_operations (operation_id,parent_kind,parent_operation_id,reservation_id,connection_id,scale_set_id) VALUES ('retirement-test','challenge',?1,?2,?3,?4)",
                (attempt.intent_id, fixture.reservation_id, fixture.scope.connection_id().as_str(), fixture.channel.scale_set_id()),
            )
            .await,
    );
    Ok(())
}

#[tokio::test]
async fn listener_delivery_claim_prevents_retirement_claim() -> Result<(), HostError> {
    let fixture = crate::journal::request_lifecycle::tests::native_pre_challenge_fixture().await?;
    crate::journal::request_lifecycle::tests::publish_native_fixture_challenge(&fixture).await?;
    let reservation_id = fixture.initialized.reservation_id;
    let delivery = fixture
        .scope
        .claim_listener_delivery(reservation_id)
        .await?
        .ok_or(HostError::Journal)?;
    assert_eq!(delivery.reservation_id(), reservation_id);

    super::candidate_tests::recover_interrupted_fixture(&fixture._journal, &fixture.database_path)
        .await?;

    assert!(
        fixture
            .scope
            .claim_post_jit_retirement(reservation_id, 60)
            .await?
            .is_none()
    );
    assert!(
        fixture
            .scope
            .recoverable_post_jit_retirements()
            .await?
            .is_empty()
    );

    let mut rows = fixture
        ._journal
        .connection()
        .await?
        .query(
            "SELECT state.delivery_state,state.jit_state,reservation.state,(SELECT COUNT(*) FROM physical_attempt_post_jit_retirement_claims claim WHERE claim.reservation_id=reservation.id),proof.proof_state,proof.deny_code,proof.challenge_state FROM capacity_reservation_provisioning_state state JOIN capacity_reservations reservation ON reservation.id=state.reservation_id JOIN connection_execution_proofs proof ON proof.reservation_id=reservation.id WHERE reservation.id=?1 AND reservation.connection_id=?2",
            (reservation_id, fixture.scope.connection_id().as_str()),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    assert_eq!(
        row.get::<String>(0).map_err(|_| HostError::Journal)?,
        "requested"
    );
    assert_eq!(
        row.get::<String>(1).map_err(|_| HostError::Journal)?,
        "bound"
    );
    assert_eq!(
        row.get::<String>(2).map_err(|_| HostError::Journal)?,
        "reserved"
    );
    assert_eq!(row.get::<i64>(3).map_err(|_| HostError::Journal)?, 0);
    assert_eq!(
        row.get::<String>(4).map_err(|_| HostError::Journal)?,
        "denied"
    );
    assert_eq!(
        row.get::<String>(5).map_err(|_| HostError::Journal)?,
        "interrupted_after_restart"
    );
    assert_eq!(
        row.get::<String>(6).map_err(|_| HostError::Journal)?,
        "abandoned"
    );
    assert!(rows.next().await.map_err(|_| HostError::Journal)?.is_none());
    Ok(())
}

#[tokio::test]
async fn startup_recovery_exposes_otherwise_eligible_retirement() -> Result<(), HostError> {
    let fixture = native_started_intake_fixture().await?;
    super::test_support::recover_fixture(&fixture).await?;
    let candidates = fixture.scope.recoverable_post_jit_retirements().await?;
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].reservation_id(), fixture.reservation_id);
    Ok(())
}

fn assert_retirement_error<T>(result: Result<T, impl std::fmt::Display>) {
    match result {
        Err(error) => assert!(error.to_string().contains(RETIREMENT_ERROR)),
        Ok(_) => panic!("The retirement trigger must reject this write."),
    }
}
