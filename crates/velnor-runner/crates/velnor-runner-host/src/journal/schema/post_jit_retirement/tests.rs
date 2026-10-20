use crate::error::HostError;
use crate::journal::request_lifecycle::tests::NativeStartedIntakeFixture;

async fn fixture_response_digest(
    fixture: &NativeStartedIntakeFixture,
) -> Result<String, HostError> {
    let connection = fixture.journal.connection().await?;
    let mut rows = connection
        .query(
            "SELECT acquire_response_digest FROM connection_request_authorizations WHERE connection_id=?1 AND scale_set_id=?2 AND request_id=?3 AND acquire_state='acquired'",
            (
                fixture.scope.connection_id().as_str(),
                fixture.channel.scale_set_id(),
                fixture.request_id,
            ),
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

async fn insert_claim(
    fixture: &NativeStartedIntakeFixture,
    acquire_response_digest: &str,
) -> Result<(), HostError> {
    let attempt = fixture
        .scope
        .current_physical_attempt(fixture.reservation_id)
        .await?
        .ok_or(HostError::Journal)?;
    let connection_id = fixture.scope.connection_id().as_str();
    let snapshot = serde_json::json!([
        "velnor-post-jit-retirement-identity-v1",
        attempt.lineage_id,
        attempt.generation,
        attempt.reservation_id,
        attempt.intent_id,
        connection_id,
        fixture.channel.scale_set_id(),
        attempt.witness_request_id,
        attempt.binding_digest,
        attempt.source_digest,
        acquire_response_digest,
        attempt.engine_id,
        attempt.docker_root_digest,
        fixture.channel.instance_binding_digest(),
        fixture.channel.runner_id(),
        fixture.channel.runner_name(),
    ])
    .to_string();
    let snapshot_digest = digest(snapshot.as_bytes());
    fixture
        .journal
        .connection()
        .await?
        .execute(
            "INSERT INTO physical_attempt_post_jit_retirement_claims (reservation_id, claim_generation, lineage_id, generation, intent_id, connection_id, scale_set_id, witness_request_id, binding_digest, source_digest, acquire_response_digest, engine_id, docker_root_digest, instance_binding_digest, runner_id, runner_name, snapshot_sha256, lease_until, created_at) VALUES (?1,1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,unixepoch()+60,unixepoch())",
            turso::params![
                attempt.reservation_id,
                attempt.lineage_id,
                attempt.generation,
                attempt.intent_id,
                connection_id,
                fixture.channel.scale_set_id(),
                attempt.witness_request_id,
                attempt.binding_digest.as_str(),
                attempt.source_digest.as_str(),
                acquire_response_digest,
                attempt.engine_id.as_str(),
                attempt.docker_root_digest.as_str(),
                fixture.channel.instance_binding_digest(),
                fixture.channel.runner_id(),
                fixture.channel.runner_name(),
                snapshot_digest.as_str(),
            ],
        )
        .await
        .map(|_| ())
        .map_err(|_| HostError::Journal)
}

#[tokio::test]
async fn acquired_witness_digest_accepts_null_attempt_creation_snapshot() -> Result<(), HostError> {
    let fixture = crate::journal::request_lifecycle::tests::native_started_intake_fixture().await?;
    let attempt = fixture
        .scope
        .current_physical_attempt(fixture.reservation_id)
        .await?
        .ok_or(HostError::Journal)?;
    assert!(attempt.acquire_response_digest.is_none());

    let response_digest = fixture_response_digest(&fixture).await?;
    insert_claim(&fixture, &response_digest).await?;
    let connection = fixture.journal.connection().await?;
    super::validate(&connection).await?;
    Ok(())
}

#[tokio::test]
async fn another_request_response_cannot_authorize_the_witness_claim() -> Result<(), HostError> {
    let fixture = crate::journal::request_lifecycle::tests::native_started_intake_fixture().await?;
    let foreign_response = serde_json::json!({
        "kind": "returned",
        "request_ids": [fixture.request_id + 1],
    })
    .to_string();
    let foreign_digest = digest(foreign_response.as_bytes());
    assert!(insert_claim(&fixture, &foreign_digest).await.is_err());
    let connection = fixture.journal.connection().await?;
    let mut rows = connection
        .query(
            "SELECT COUNT(*) FROM physical_attempt_post_jit_retirement_claims WHERE reservation_id=?1",
            [fixture.reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let count = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<i64>(0)
        .map_err(|_| HostError::Journal)?;
    assert_eq!(count, 0);
    Ok(())
}

fn digest(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};

    let mut output = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        use std::fmt::Write as _;
        let _written = write!(output, "{byte:02x}");
    }
    output
}
