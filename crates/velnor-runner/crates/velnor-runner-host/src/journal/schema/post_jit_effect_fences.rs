//! This module checks database triggers that reject specified operations after a retirement claim.

use crate::error::HostError;

const JIT_CLAIM_FENCE: &str = "CREATE TRIGGER post_jit_jit_claim_fence BEFORE INSERT ON connection_runner_jits WHEN NEW.state='claimed' AND EXISTS(SELECT 1 FROM physical_attempt_current cur JOIN physical_attempts a ON a.lineage_id=cur.lineage_id AND a.generation=cur.generation AND a.reservation_id=cur.reservation_id AND a.intent_id=cur.intent_id JOIN physical_attempt_post_jit_retirement_claims c ON c.lineage_id=a.lineage_id AND c.generation=a.generation AND c.reservation_id=a.reservation_id WHERE a.intent_id=NEW.intent_id AND a.connection_id=NEW.connection_id AND a.scale_set_id=NEW.scale_set_id) BEGIN SELECT RAISE(ABORT,'A retirement claim prevents this operation.'); END";
const RUNNER_START_FENCE: &str = "CREATE TRIGGER post_jit_runner_start_fence BEFORE UPDATE OF state ON connection_runner_jits WHEN NEW.state='start_requested' AND EXISTS(SELECT 1 FROM physical_attempts a JOIN physical_attempt_post_jit_retirement_claims c ON c.lineage_id=a.lineage_id AND c.generation=a.generation AND c.reservation_id=a.reservation_id WHERE a.intent_id=NEW.intent_id AND a.connection_id=NEW.connection_id AND a.scale_set_id=NEW.scale_set_id) BEGIN SELECT RAISE(ABORT,'A retirement claim prevents this operation.'); END";

const TRIGGERS: [&str; 8] = [
    JIT_CLAIM_FENCE,
    RUNNER_START_FENCE,
    "CREATE TRIGGER post_jit_reservation_jit_fence BEFORE UPDATE OF jit_state ON capacity_reservation_provisioning_state WHEN NEW.jit_state='requested' AND EXISTS(SELECT 1 FROM physical_attempt_post_jit_retirement_claims c WHERE c.reservation_id=NEW.reservation_id) BEGIN SELECT RAISE(ABORT,'A retirement claim prevents this operation.'); END",
    "CREATE TRIGGER post_jit_job_facts_fence BEFORE UPDATE OF publication_state ON capacity_reservation_provisioning_state WHEN NEW.publication_state='requested' AND EXISTS(SELECT 1 FROM physical_attempt_post_jit_retirement_claims c WHERE c.reservation_id=NEW.reservation_id) BEGIN SELECT RAISE(ABORT,'A retirement claim prevents this operation.'); END",
    "CREATE TRIGGER post_jit_listener_fence BEFORE UPDATE OF delivery_state ON capacity_reservation_provisioning_state WHEN NEW.delivery_state='requested' AND EXISTS(SELECT 1 FROM physical_attempt_post_jit_retirement_claims c WHERE c.reservation_id=NEW.reservation_id) BEGIN SELECT RAISE(ABORT,'A retirement claim prevents this operation.'); END",
    "CREATE TRIGGER post_jit_challenge_fence BEFORE UPDATE OF challenge_state ON connection_execution_proofs WHEN NEW.challenge_state='requested' AND EXISTS(SELECT 1 FROM physical_attempt_post_jit_retirement_claims c WHERE c.reservation_id=NEW.reservation_id AND c.connection_id=NEW.connection_id AND c.scale_set_id=NEW.scale_set_id) BEGIN SELECT RAISE(ABORT,'A retirement claim prevents this operation.'); END",
    "CREATE TRIGGER post_jit_decision_fence BEFORE INSERT ON connection_execution_decisions WHEN EXISTS(SELECT 1 FROM physical_attempt_post_jit_retirement_claims c WHERE c.reservation_id=NEW.reservation_id AND c.connection_id=NEW.connection_id AND c.scale_set_id=NEW.scale_set_id) BEGIN SELECT RAISE(ABORT,'A retirement claim prevents this operation.'); END",
    "CREATE TRIGGER post_jit_publication_fence BEFORE INSERT ON execution_publication_operations WHEN EXISTS(SELECT 1 FROM physical_attempt_post_jit_retirement_claims c WHERE c.reservation_id=NEW.reservation_id AND c.connection_id=NEW.connection_id AND c.scale_set_id=NEW.scale_set_id) BEGIN SELECT RAISE(ABORT,'A retirement claim prevents this operation.'); END",
];

pub(super) async fn restore_jit_attached(conn: &turso::Connection) -> Result<(), HostError> {
    for sql in [JIT_CLAIM_FENCE, RUNNER_START_FENCE] {
        conn.execute(sql, ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    Ok(())
}

pub(super) async fn bootstrap(conn: &turso::Connection) -> Result<(), HostError> {
    for sql in TRIGGERS {
        conn.execute(sql, ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    Ok(())
}

pub(super) async fn validate(conn: &turso::Connection) -> Result<(), HostError> {
    for expected_sql in TRIGGERS {
        let name = trigger_name(expected_sql);
        let mut rows = conn
            .query(
                "SELECT sql FROM sqlite_schema WHERE type='trigger' AND name=?1",
                (name,),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        let actual: String = rows
            .next()
            .await
            .map_err(|_| HostError::Journal)?
            .ok_or(HostError::Journal)?
            .get(0)
            .map_err(|_| HostError::Journal)?;
        if rows.next().await.map_err(|_| HostError::Journal)?.is_some()
            || super::sql_normalization::normalize(&actual)
                != super::sql_normalization::normalize(expected_sql)
        {
            return Err(HostError::Journal);
        }
    }
    Ok(())
}

fn trigger_name(sql: &str) -> &str {
    sql.strip_prefix("CREATE TRIGGER ")
        .and_then(|body| body.split_ascii_whitespace().next())
        .unwrap_or("")
}
