use super::*;

#[tokio::test]
async fn published_proof_reopens_with_its_historical_bindings() -> Result<(), HostError> {
    let fixture = crate::journal::request_lifecycle::tests::native_pre_challenge_fixture().await?;
    crate::journal::request_lifecycle::tests::publish_native_fixture_challenge(&fixture).await?;
    let reservation_id = fixture.initialized.reservation_id;
    let database_path = fixture.database_path.clone();
    let crate::journal::request_lifecycle::tests::NativePreChallengeFixture {
        _scratch: scratch,
        journal,
        ..
    } = fixture;
    drop(journal);

    let reopened = crate::journal::Journal::open(&database_path).await?;
    let connection = reopened.connection().await?;
    let mut rows = connection
        .query(
            "SELECT proof.proof_state, proof.binding_digest = reservation.binding_digest, proof.binding_digest = policy.binding_digest FROM connection_execution_proofs AS proof JOIN capacity_reservations AS reservation ON reservation.id = proof.reservation_id AND reservation.intent_id = proof.intent_id AND reservation.connection_id = proof.connection_id JOIN connection_job_authority_policies AS policy ON policy.connection_id = proof.connection_id AND policy.generation = proof.generation AND policy.policy_digest = proof.policy_digest WHERE proof.reservation_id = ?1",
            [reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    let state: String = row.get(0).map_err(|_| HostError::Journal)?;
    let reservation_matches: i64 = row.get(1).map_err(|_| HostError::Journal)?;
    let policy_matches: i64 = row.get(2).map_err(|_| HostError::Journal)?;
    if state != "pending"
        || reservation_matches != 1
        || policy_matches != 1
        || rows.next().await.map_err(|_| HostError::Journal)?.is_some()
    {
        return Err(HostError::Journal);
    }
    drop(connection);
    drop(reopened);
    drop(scratch);
    Ok(())
}

#[tokio::test]
async fn reopen_rejects_a_well_formed_proof_binding_mismatch() -> Result<(), HostError> {
    let fixture = crate::journal::request_lifecycle::tests::native_pre_challenge_fixture().await?;
    crate::journal::request_lifecycle::tests::publish_native_fixture_challenge(&fixture).await?;
    let reservation_id = fixture.initialized.reservation_id;
    let database_path = fixture.database_path.clone();
    let connection = fixture.journal.connection().await?;
    let original_binding = proof_binding(&connection, reservation_id).await?;
    let replacement = different_digest(&original_binding)?;
    let original_guard = transition_guard(&connection).await?;
    connection
        .execute("DROP TRIGGER execution_proof_transition_guard", ())
        .await
        .map_err(|_| HostError::Journal)?;
    let changed = connection
        .execute(
            "UPDATE connection_execution_proofs SET binding_digest = ?1 WHERE reservation_id = ?2",
            (replacement.as_str(), reservation_id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    if changed != 1 {
        return Err(HostError::Journal);
    }
    connection
        .execute(&original_guard, ())
        .await
        .map_err(|_| HostError::Journal)?;
    drop(connection);
    let crate::journal::request_lifecycle::tests::NativePreChallengeFixture {
        _scratch: scratch,
        journal,
        ..
    } = fixture;
    drop(journal);

    let reopen = crate::journal::Journal::open(&database_path).await;
    if reopen.is_ok() {
        return Err(HostError::Journal);
    }
    drop(scratch);
    Ok(())
}

async fn proof_binding(
    connection: &turso::Connection,
    reservation_id: i64,
) -> Result<String, HostError> {
    let mut rows = connection
        .query(
            "SELECT binding_digest FROM connection_execution_proofs WHERE reservation_id = ?1",
            [reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    rows.next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get(0)
        .map_err(|_| HostError::Journal)
}

async fn transition_guard(connection: &turso::Connection) -> Result<String, HostError> {
    let mut rows = connection
        .query(
            "SELECT sql FROM sqlite_schema WHERE type = 'trigger' AND name = 'execution_proof_transition_guard'",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    rows.next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get(0)
        .map_err(|_| HostError::Journal)
}

fn different_digest(value: &str) -> Result<String, HostError> {
    let mut bytes = value.as_bytes().to_vec();
    let first = bytes.first_mut().ok_or(HostError::Journal)?;
    *first = if *first == b'0' { b'1' } else { b'0' };
    String::from_utf8(bytes).map_err(|_| HostError::Journal)
}
