#[path = "post_jit_retirement/fixture_stack_cases.rs"]
mod fixture_stack_cases;

use crate::error::HostError;
use crate::journal::connection::storage::{in_transaction, read_transaction};
use crate::journal::{Journal, MetadataSettlementResult};
use sha2::{Digest, Sha256};

use super::{NativeStartedIntakeFixture, helpers};

#[path = "post_jit_retirement/digest_reopen.rs"]
mod digest_reopen;
#[path = "post_jit_retirement/late_events.rs"]
pub(crate) mod late_events;
#[path = "post_jit_retirement/reciprocal.rs"]
mod reciprocal;

#[test]
fn retirement_settlement_writes_terminal_before_release_and_reopens() -> Result<(), String> {
    crate::launch::test_harness_stack::run_on_test_harness_stack(
        "retirement_settlement_writes_terminal_before_release_and_reopens",
        || {
            Box::pin(async {
                fixture_stack_cases::retirement_settlement_writes_terminal_before_release_and_reopens_on_fixture_stack()
                    .await
                    .map_err(|error| error.to_string())
            })
        },
    )
}

#[test]
fn retirement_settlement_rolls_back_terminal_and_release_when_current_delete_fails()
-> Result<(), String> {
    crate::launch::test_harness_stack::run_on_test_harness_stack(
        "retirement_settlement_rolls_back_terminal_and_release_when_current_delete_fails",
        || {
            Box::pin(async {
                fixture_stack_cases::retirement_settlement_rolls_back_terminal_and_release_when_current_delete_fails_on_fixture_stack().await.map_err(|error| error.to_string())
            })
        },
    )
}

#[tokio::test]
async fn another_runner_event_on_the_acquire_witness_does_not_block_retirement_claim()
-> Result<(), HostError> {
    let fixture = prepared_fixture().await?;
    record_witness_runner_event(&fixture, false).await?;
    seed_mocked_external_evidence(&fixture).await?;
    let scope = fixture.scope.clone();
    read_transaction(scope.journal_ref(), async |connection| {
        assert_row(
            connection,
            "SELECT COUNT(*) FROM physical_attempt_post_jit_retirement_claims WHERE reservation_id=?1",
            fixture.reservation_id,
            1,
        )
        .await
    })
    .await
}

#[tokio::test]
async fn event_for_the_retiring_runner_still_blocks_retirement_claim() -> Result<(), HostError> {
    let fixture = prepared_fixture().await?;
    record_witness_runner_event(&fixture, true).await?;
    assert!(seed_mocked_external_evidence(&fixture).await.is_err());
    Ok(())
}

async fn record_witness_runner_event(
    fixture: &NativeStartedIntakeFixture,
    same_runner: bool,
) -> Result<(), HostError> {
    let attempt = fixture
        .scope
        .current_physical_attempt(fixture.reservation_id)
        .await?
        .ok_or(HostError::Journal)?;
    let scope = fixture.scope.clone();
    in_transaction(scope.journal_ref(), async |connection| {
        let mut rows = connection
            .query(
                "SELECT runner_id,runner_name FROM connection_runner_jits WHERE intent_id=?1 AND connection_id=?2",
                (attempt.intent_id, scope.connection_id().as_str()),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        let row = rows
            .next()
            .await
            .map_err(|_| HostError::Journal)?
            .ok_or(HostError::Journal)?;
        let runner_id: i64 = row.get(0).map_err(|_| HostError::Journal)?;
        let runner_name: String = row.get(1).map_err(|_| HostError::Journal)?;
        if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
            return Err(HostError::Journal);
        }
        let event_runner_id = if same_runner { runner_id } else { runner_id + 1 };
        let event_runner_name = if same_runner {
            runner_name
        } else {
            format!("{runner_name}-previous")
        };
        let event_digest = digest(b"test-only durable event identity for witness assignment");
        let changed = connection
            .execute(
                "UPDATE connection_request_authorizations SET started_message_id=9001,started_event_digest=?1,started_job_id='previous-job',started_runner_id=?2,started_runner_name=?3 WHERE connection_id=?4 AND scale_set_id=?5 AND request_id=?6 AND acquire_state='acquired'",
                (event_digest.as_str(), event_runner_id, event_runner_name.as_str(), scope.connection_id().as_str(), fixture.runner.runner_scale_set_id, attempt.witness_request_id),
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

async fn prepared_fixture() -> Result<NativeStartedIntakeFixture, HostError> {
    let fixture = super::native_started_intake_fixture().await?;
    crate::journal::connection::reservation::post_jit_retirement::test_support::recover_fixture(
        &fixture,
    )
    .await?;
    Ok(fixture)
}

async fn seed_mocked_external_evidence(
    fixture: &NativeStartedIntakeFixture,
) -> Result<(), HostError> {
    let attempt = fixture
        .scope
        .current_physical_attempt(fixture.reservation_id)
        .await?
        .ok_or(HostError::Journal)?;
    let scope = fixture.scope.clone();
    let reservation_id = fixture.reservation_id;
    let claim = scope
        .claim_post_jit_retirement(reservation_id, 60)
        .await?
        .ok_or(HostError::Journal)?;
    let claim_generation = claim.claim_generation();
    in_transaction(scope.journal_ref(), async |connection| {
        let mut rows = connection.query(
            "SELECT auth.acquire_response_digest,state.instance_binding_digest,j.runner_id,j.runner_name FROM connection_request_authorizations auth JOIN capacity_reservation_provisioning_state state ON state.reservation_id=?1 JOIN connection_runner_jits j ON j.intent_id=?2 AND j.connection_id=?3 WHERE auth.connection_id=?3 AND auth.scale_set_id=?4 AND auth.request_id=?5 AND auth.acquire_state='acquired'",
            (reservation_id, attempt.intent_id, scope.connection_id().as_str(), fixture.runner.runner_scale_set_id, attempt.witness_request_id),
        ).await.map_err(|_| HostError::Journal)?;
        let row = rows.next().await.map_err(|_| HostError::Journal)?.ok_or(HostError::Journal)?;
        let acquire_digest: String = row.get(0).map_err(|_| HostError::Journal)?;
        let instance_binding: String = row.get(1).map_err(|_| HostError::Journal)?;
        let runner_id: i64 = row.get(2).map_err(|_| HostError::Journal)?;
        let runner_name: String = row.get(3).map_err(|_| HostError::Journal)?;
        if rows.next().await.map_err(|_| HostError::Journal)?.is_some() { return Err(HostError::Journal); }
        let snapshot = identity_snapshot(&RetirementIdentitySnapshot {
            lineage_id: attempt.lineage_id,
            generation: attempt.generation,
            reservation_id,
            intent_id: attempt.intent_id,
            connection_id: scope.connection_id().as_str(),
            scale_set_id: fixture.runner.runner_scale_set_id,
            witness_request_id: attempt.witness_request_id,
            binding: attempt.binding_digest.as_str(),
            source: attempt.source_digest.as_str(),
            acquire: acquire_digest.as_str(),
            engine: attempt.engine_id.as_str(),
            root: attempt.docker_root_digest.as_str(),
            instance_binding: instance_binding.as_str(),
            runner_id,
            runner_name: runner_name.as_str(),
        })?;
        let mut rows = connection
            .query(
                "SELECT snapshot_sha256 FROM physical_attempt_post_jit_retirement_claims WHERE reservation_id=?1 AND claim_generation=?2",
                (reservation_id, claim_generation),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        let row = rows
            .next()
            .await
            .map_err(|_| HostError::Journal)?
            .ok_or(HostError::Journal)?;
        let persisted_snapshot: String = row.get(0).map_err(|_| HostError::Journal)?;
        assert_eq!(persisted_snapshot, snapshot);
        if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
            return Err(HostError::Journal);
        }
        let mocked_stop = digest(b"mocked runner stop evidence for storage-only retirement test");
        insert_phase(connection, reservation_id, 1, "runner_stopped", mocked_stop.as_str()).await?;
        let request_digest = digest(b"mocked external runner lookup request");
        let response_digest = digest(b"mocked external empty runner lookup response");
        connection.execute(
            "INSERT INTO physical_attempt_post_jit_retirement_remote_absence (reservation_id,claim_generation,runner_id,runner_name,scale_set_id,lookup_status,lookup_count,lookup_value_count,request_sha256,response_sha256,remove_status,observed_at_unix_ms) VALUES (?1,1,?2,?3,?4,200,0,0,?5,?6,NULL,CAST((julianday('now')-2440587.5)*86400000 AS INTEGER))",
            (reservation_id, runner_id, runner_name.as_str(), fixture.runner.runner_scale_set_id, request_digest.as_str(), response_digest.as_str()),
        ).await.map_err(|_| HostError::Journal)?;
        let mocked_absence = digest(b"mocked external runner absence receipt");
        insert_phase(connection, reservation_id, 2, "remote_absent", mocked_absence.as_str()).await?;
        Ok(())
    }).await
}

async fn record_storage_and_network_proofs(
    fixture: &NativeStartedIntakeFixture,
) -> Result<(), HostError> {
    helpers::record_observed_storage_cleanup(&fixture.scope, fixture.reservation_id).await?;
    let storage = digest(b"real cleanup observer and Docker storage absence APIs completed");
    append_phase(fixture, 3, "storage_absent", storage.as_str()).await?;
    helpers::record_owned_network_cleanup(&fixture.scope, fixture.reservation_id).await?;
    let network = digest(b"network removal was recorded through the scoped journal API");
    append_phase(fixture, 4, "network_absent", network.as_str()).await?;
    fixture
        .scope
        .record_remote_cleanup(fixture.reservation_id)
        .await?;
    fixture
        .scope
        .record_local_cleanup(fixture.reservation_id)
        .await
}

async fn append_phase(
    fixture: &NativeStartedIntakeFixture,
    index: i64,
    phase: &str,
    evidence: &str,
) -> Result<(), HostError> {
    let scope = fixture.scope.clone();
    in_transaction(scope.journal_ref(), async |connection| {
        insert_phase(connection, fixture.reservation_id, index, phase, evidence).await
    })
    .await
}

async fn insert_phase(
    connection: &turso::Connection,
    reservation_id: i64,
    index: i64,
    phase: &str,
    evidence: &str,
) -> Result<(), HostError> {
    let changed = connection.execute(
        "INSERT INTO physical_attempt_post_jit_retirement_progress (reservation_id,claim_generation,phase_index,phase,evidence_sha256,created_at) VALUES (?1,1,?2,?3,?4,unixepoch())",
        (reservation_id, index, phase, evidence),
    ).await.map_err(|_| HostError::Journal)?;
    if changed == 1 {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}

struct RetirementIdentitySnapshot<'a> {
    lineage_id: i64,
    generation: i64,
    reservation_id: i64,
    intent_id: i64,
    connection_id: &'a str,
    scale_set_id: i64,
    witness_request_id: i64,
    binding: &'a str,
    source: &'a str,
    acquire: &'a str,
    engine: &'a str,
    root: &'a str,
    instance_binding: &'a str,
    runner_id: i64,
    runner_name: &'a str,
}

fn identity_snapshot(identity: &RetirementIdentitySnapshot<'_>) -> Result<String, HostError> {
    let value = serde_json::json!([
        "velnor-post-jit-retirement-identity-v1",
        identity.lineage_id,
        identity.generation,
        identity.reservation_id,
        identity.intent_id,
        identity.connection_id,
        identity.scale_set_id,
        identity.witness_request_id,
        identity.binding,
        identity.source,
        identity.acquire,
        identity.engine,
        identity.root,
        identity.instance_binding,
        identity.runner_id,
        identity.runner_name,
    ]);
    let bytes = serde_json::to_vec(&value).map_err(|_| HostError::Journal)?;
    Ok(digest(&bytes))
}

fn digest(bytes: &[u8]) -> String {
    let mut digest = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        use std::fmt::Write as _;
        write!(digest, "{byte:02x}").expect("writing to a String must succeed");
    }
    digest
}

async fn assert_terminal_state(fixture: &NativeStartedIntakeFixture) -> Result<(), HostError> {
    let scope = fixture.scope.clone();
    read_transaction(scope.journal_ref(), async |connection| {
        assert_row(connection, "SELECT COUNT(*) FROM physical_attempt_post_jit_retirements WHERE reservation_id=?1", fixture.reservation_id, 1).await?;
        assert_row(connection, "SELECT COUNT(*) FROM physical_attempt_outcomes WHERE reservation_id=?1", fixture.reservation_id, 0).await?;
        assert_row(connection, "SELECT COUNT(*) FROM physical_attempt_current WHERE reservation_id=?1", fixture.reservation_id, 0).await?;
        assert_row(connection, "SELECT COUNT(*) FROM physical_attempt_post_jit_retirement_progress WHERE reservation_id=?1 AND phase_index=5 AND phase='capacity_settled'", fixture.reservation_id, 1).await?;
        assert_row(connection, "SELECT COUNT(*) FROM capacity_reservations WHERE id=?1 AND state='released' AND remote_cleanup_proven=1 AND local_cleanup_proven=1", fixture.reservation_id, 1).await
    }).await
}

async fn assert_reopened_terminal_state(
    journal: &Journal,
    fixture: &NativeStartedIntakeFixture,
) -> Result<(), HostError> {
    read_transaction(journal, async |connection| {
        assert_row(connection, "SELECT COUNT(*) FROM physical_attempt_terminal_proofs WHERE reservation_id=?1 AND outcome='post_jit_retired'", fixture.reservation_id, 1).await?;
        assert_row(connection, "SELECT COUNT(*) FROM physical_attempt_current WHERE reservation_id=?1", fixture.reservation_id, 0).await
    }).await
}

async fn assert_held_without_terminal(
    fixture: &NativeStartedIntakeFixture,
) -> Result<(), HostError> {
    let scope = fixture.scope.clone();
    read_transaction(scope.journal_ref(), async |connection| {
        assert_row(connection, "SELECT COUNT(*) FROM physical_attempt_post_jit_retirements WHERE reservation_id=?1", fixture.reservation_id, 0).await?;
        assert_row(connection, "SELECT COUNT(*) FROM physical_attempt_post_jit_retirement_progress WHERE reservation_id=?1 AND phase_index=5", fixture.reservation_id, 0).await?;
        assert_row(connection, "SELECT COUNT(*) FROM capacity_reservations WHERE id=?1 AND state!='released'", fixture.reservation_id, 1).await?;
        assert_row(connection, "SELECT COUNT(*) FROM physical_attempt_current WHERE reservation_id=?1", fixture.reservation_id, 1).await
    }).await
}

async fn assert_row(
    connection: &turso::Connection,
    sql: &str,
    reservation_id: i64,
    expected: i64,
) -> Result<(), HostError> {
    let mut rows = connection
        .query(sql, [reservation_id])
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    let actual: i64 = row.get(0).map_err(|_| HostError::Journal)?;
    if actual != expected || rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        return Err(HostError::Journal);
    }
    Ok(())
}

async fn install_current_delete_failure(
    fixture: &NativeStartedIntakeFixture,
) -> Result<(), HostError> {
    let scope = fixture.scope.clone();
    in_transaction(scope.journal_ref(), async |connection| {
        let sql = format!("CREATE TRIGGER test_fail_retirement_current_delete BEFORE DELETE ON physical_attempt_current WHEN OLD.reservation_id={} BEGIN SELECT RAISE(ABORT,'injected retirement rollback'); END", fixture.reservation_id);
        connection.execute(&sql, ()).await.map_err(|_| HostError::Journal)?;
        Ok(())
    }).await
}

async fn drop_current_delete_failure(
    fixture: &NativeStartedIntakeFixture,
) -> Result<(), HostError> {
    let scope = fixture.scope.clone();
    in_transaction(scope.journal_ref(), async |connection| {
        connection
            .execute("DROP TRIGGER test_fail_retirement_current_delete", ())
            .await
            .map_err(|_| HostError::Journal)?;
        Ok(())
    })
    .await
}
