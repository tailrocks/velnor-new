use crate::error::HostError;
use crate::journal::{Journal, request_lifecycle::tests::native_started_intake_fixture};
use std::collections::VecDeque;
use velnor_runner_github::{Exchange, Transport, TransportFail, ensure_runner_absent};

#[path = "v37_migration_tests/progressed.rs"]
mod progressed;

#[tokio::test]
async fn v36_live_claim_survives_v37_migration_and_reopen() -> Result<(), HostError> {
    let fixture = native_started_intake_fixture().await?;
    let connection = fixture.journal.connection().await?;
    downgrade_retirement_guard_to_v36(&connection).await?;
    let (reservation_id, snapshot_digest) =
        insert_v36_live_claim(&connection, &fixture, 60).await?;
    super::validate(&connection).await?;
    let before = claim_rows(&connection, reservation_id).await?;
    assert_eq!(before.0, 1);
    assert_eq!(before.2, snapshot_digest);
    assert_eq!(before.3, "claimed");
    drop(connection);

    let path = fixture.database_path.clone();
    let crate::journal::request_lifecycle::tests::NativeStartedIntakeFixture { _scratch, .. } =
        fixture;
    let migrated = Journal::open(&path).await?;
    let connection = migrated.connection().await?;
    assert_eq!(scalar(&connection, "PRAGMA user_version").await?, 37);
    assert_eq!(claim_rows(&connection, reservation_id).await?, before);
    super::validate_v37(&connection).await?;
    drop(connection);
    drop(migrated);

    let reopened = Journal::open(&path).await?;
    let connection = reopened.connection().await?;
    assert_eq!(scalar(&connection, "PRAGMA user_version").await?, 37);
    assert_eq!(claim_rows(&connection, reservation_id).await?, before);
    Ok(())
}

#[tokio::test]
async fn v36_to_v37_guard_replacement_rolls_back_atomically() -> Result<(), HostError> {
    let fixture = native_started_intake_fixture().await?;
    let connection = fixture.journal.connection().await?;
    downgrade_retirement_guard_to_v36(&connection).await?;
    connection
        .execute("BEGIN IMMEDIATE", ())
        .await
        .map_err(|_| HostError::Journal)?;
    super::super::migration::from_version_thirty_six(&connection).await?;
    assert_eq!(scalar(&connection, "PRAGMA user_version").await?, 37);
    super::validate_v37(&connection).await?;
    connection
        .execute("ROLLBACK", ())
        .await
        .map_err(|_| HostError::Journal)?;
    assert_eq!(scalar(&connection, "PRAGMA user_version").await?, 36);
    super::validate(&connection).await?;
    Ok(())
}

async fn insert_v36_live_claim(
    connection: &turso::Connection,
    fixture: &crate::journal::request_lifecycle::tests::NativeStartedIntakeFixture,
    lease_seconds: i64,
) -> Result<(i64, String), HostError> {
    let attempt = fixture
        .scope
        .current_physical_attempt(fixture.reservation_id)
        .await?
        .ok_or(HostError::Journal)?;
    let response_digest = acquired_response_digest(connection, fixture).await?;
    let snapshot = serde_json::json!([
        "velnor-post-jit-retirement-identity-v1",
        attempt.lineage_id,
        attempt.generation,
        attempt.reservation_id,
        attempt.intent_id,
        fixture.scope.connection_id().as_str(),
        fixture.channel.scale_set_id(),
        attempt.witness_request_id,
        attempt.binding_digest,
        attempt.source_digest,
        response_digest,
        attempt.engine_id,
        attempt.docker_root_digest,
        fixture.channel.instance_binding_digest(),
        fixture.channel.runner_id(),
        fixture.channel.runner_name(),
    ])
    .to_string();
    let snapshot_digest = digest(snapshot.as_bytes());
    connection
        .execute(
            "INSERT INTO physical_attempt_post_jit_retirement_claims (reservation_id,claim_generation,lineage_id,generation,intent_id,connection_id,scale_set_id,witness_request_id,binding_digest,source_digest,acquire_response_digest,engine_id,docker_root_digest,instance_binding_digest,runner_id,runner_name,snapshot_sha256,lease_until,created_at) VALUES (?1,1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,unixepoch()+?17,unixepoch())",
            turso::params![
                attempt.reservation_id,
                attempt.lineage_id,
                attempt.generation,
                attempt.intent_id,
                fixture.scope.connection_id().as_str(),
                fixture.channel.scale_set_id(),
                attempt.witness_request_id,
                attempt.binding_digest.as_str(),
                attempt.source_digest.as_str(),
                response_digest.as_str(),
                attempt.engine_id.as_str(),
                attempt.docker_root_digest.as_str(),
                fixture.channel.instance_binding_digest(),
                fixture.channel.runner_id(),
                fixture.channel.runner_name(),
                snapshot_digest.as_str(),
                lease_seconds,
            ],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "INSERT INTO physical_attempt_post_jit_retirement_progress (reservation_id,claim_generation,phase_index,phase,evidence_sha256,created_at) VALUES (?1,1,0,'claimed',?2,unixepoch())",
            (attempt.reservation_id, snapshot_digest.as_str()),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    Ok((attempt.reservation_id, snapshot_digest))
}

async fn assert_phase3_is_charged(
    journal: &Journal,
    reservation_id: i64,
    generation: i64,
    snapshot: &str,
) -> Result<(), HostError> {
    let connection = journal.connection().await?;
    let mut rows = connection
        .query(
            "SELECT reservation.state,reservation.local_cleanup_proven,state.storage_cleanup_proven,claim.snapshot_sha256,(SELECT COUNT(*) FROM physical_attempt_post_jit_retirement_progress phase WHERE phase.reservation_id=claim.reservation_id AND phase.claim_generation=claim.claim_generation AND phase.phase_index=3 AND phase.phase='storage_absent') FROM capacity_reservations reservation JOIN capacity_reservation_provisioning_state state ON state.reservation_id=reservation.id JOIN physical_attempt_post_jit_retirement_claims claim ON claim.reservation_id=reservation.id AND claim.claim_generation=?2 WHERE reservation.id=?1",
            (reservation_id, generation),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    let reservation_state: String = row.get(0).map_err(|_| HostError::Journal)?;
    let local_proof: i64 = row.get(1).map_err(|_| HostError::Journal)?;
    let storage_proof: i64 = row.get(2).map_err(|_| HostError::Journal)?;
    let observed_snapshot: String = row.get(3).map_err(|_| HostError::Journal)?;
    let phase_rows: i64 = row.get(4).map_err(|_| HostError::Journal)?;
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some()
        || !matches!(
            reservation_state.as_str(),
            "reserved" | "uncertain" | "recovering"
        )
        || local_proof != 1
        || storage_proof != 1
        || observed_snapshot != snapshot
        || phase_rows != 1
    {
        return Err(HostError::Identity);
    }
    Ok(())
}

struct EmptyRunnerTransport;

impl Transport for EmptyRunnerTransport {
    fn exchange(
        &mut self,
        _request: &velnor_runner_github::SessionRequest,
    ) -> Result<Exchange, TransportFail> {
        Ok(Exchange {
            status: 200,
            body: br#"{"count":0,"value":[]}"#.to_vec(),
        })
    }
}

#[test]
fn v37_first_claim_guard_requires_exact_interrupted_recovery_marker() -> Result<(), HostError> {
    let v36 = super::guards::CLAIM_INSERT_GUARD;
    let v37 = super::guards::claim_insert_guard_v37()?;
    assert!(v36.contains("proof.proof_state='pending'"));
    assert!(v36.contains("proof.challenge_state='published'"));
    assert!(!v36.contains("r.state='recovering'"));
    assert!(v37.contains("r.state='recovering'"));
    assert!(v37.contains("proof.proof_state='denied'"));
    assert!(v37.contains("proof.deny_code='interrupted_after_restart'"));
    assert!(v37.contains("proof.challenge_state='abandoned'"));
    assert!(v37.contains("NEW.claim_generation>1 AND prior.lease_until<=unixepoch()"));
    Ok(())
}

async fn downgrade_retirement_guard_to_v36(
    connection: &turso::Connection,
) -> Result<(), HostError> {
    connection
        .execute(
            "DROP TRIGGER physical_attempt_post_jit_retirement_claim_insert_guard",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(super::guards::CLAIM_INSERT_GUARD, ())
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute("PRAGMA user_version=36", ())
        .await
        .map_err(|_| HostError::Journal)?;
    super::validate(connection).await
}

async fn acquired_response_digest(
    connection: &turso::Connection,
    fixture: &crate::journal::request_lifecycle::tests::NativeStartedIntakeFixture,
) -> Result<String, HostError> {
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

async fn claim_rows(
    connection: &turso::Connection,
    reservation_id: i64,
) -> Result<(i64, i64, String, String), HostError> {
    let mut rows = connection
        .query(
            "SELECT COUNT(*),MAX(claim.claim_generation),MAX(claim.snapshot_sha256),MAX(progress.phase) FROM physical_attempt_post_jit_retirement_claims claim LEFT JOIN physical_attempt_post_jit_retirement_progress progress ON progress.reservation_id=claim.reservation_id AND progress.claim_generation=claim.claim_generation WHERE claim.reservation_id=?1",
            [reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    let result = (
        row.get(0).map_err(|_| HostError::Journal)?,
        row.get(1).map_err(|_| HostError::Journal)?,
        row.get(2).map_err(|_| HostError::Journal)?,
        row.get(3).map_err(|_| HostError::Journal)?,
    );
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        return Err(HostError::Journal);
    }
    Ok(result)
}

async fn scalar(connection: &turso::Connection, sql: &str) -> Result<i64, HostError> {
    let mut rows = connection
        .query(sql, ())
        .await
        .map_err(|_| HostError::Journal)?;
    rows.next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get(0)
        .map_err(|_| HostError::Journal)
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
