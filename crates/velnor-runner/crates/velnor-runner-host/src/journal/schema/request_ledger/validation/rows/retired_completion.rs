use crate::error::HostError;

use super::AuthorizationIdentity;

pub(super) async fn validate(
    conn: &turso::Connection,
    authorization: &AuthorizationIdentity,
    intent_id: i64,
    runner_id: Option<i64>,
    runner_name: Option<&str>,
) -> Result<(), HostError> {
    let (Some(runner_id), Some(runner_name)) = (runner_id, runner_name) else {
        return Err(HostError::Journal);
    };
    let mut rows = conn
        .query(
            "SELECT COUNT(*) FROM physical_attempt_post_jit_retirement_claims claim JOIN physical_attempts attempt ON attempt.lineage_id=claim.lineage_id AND attempt.generation=claim.generation AND attempt.reservation_id=claim.reservation_id AND attempt.intent_id=claim.intent_id JOIN physical_attempt_lineages lineage ON lineage.lineage_id=attempt.lineage_id AND lineage.connection_id=attempt.connection_id AND lineage.scale_set_id=attempt.scale_set_id AND lineage.binding_digest=attempt.binding_digest JOIN connection_intents intent ON intent.id=claim.intent_id AND intent.connection_id=claim.connection_id AND intent.scale_set_id=claim.scale_set_id AND intent.kind='launch' AND intent.runner_name=claim.runner_name AND intent.github_runner_id=CAST(claim.runner_id AS TEXT) JOIN connection_runner_jits jit ON jit.intent_id=claim.intent_id AND jit.connection_id=claim.connection_id AND jit.scale_set_id=claim.scale_set_id AND jit.witness_request_id=claim.witness_request_id AND jit.runner_id=claim.runner_id AND jit.runner_name=claim.runner_name JOIN connection_request_authorizations witness ON witness.connection_id=claim.connection_id AND witness.scale_set_id=claim.scale_set_id AND witness.request_id=claim.witness_request_id AND witness.binding_digest=claim.binding_digest AND witness.source_digest=claim.source_digest AND witness.acquire_response_digest=claim.acquire_response_digest AND witness.acquire_state='acquired' WHERE claim.connection_id=?1 AND claim.scale_set_id=?2 AND claim.binding_digest=?3 AND claim.intent_id=?4 AND claim.runner_id=?5 AND claim.runner_name=?6 AND claim.claim_generation=(SELECT MAX(latest.claim_generation) FROM physical_attempt_post_jit_retirement_claims latest WHERE latest.reservation_id=claim.reservation_id) AND attempt.connection_id=claim.connection_id AND attempt.scale_set_id=claim.scale_set_id AND attempt.binding_digest=claim.binding_digest AND attempt.witness_request_id=claim.witness_request_id AND attempt.source_digest=claim.source_digest AND attempt.intent_id=?4 AND lineage.connection_id=?1 AND lineage.scale_set_id=?2 AND lineage.binding_digest=?3 AND jit.intent_id=?4 AND jit.runner_id=?5 AND jit.runner_name=?6 AND EXISTS (SELECT 1 FROM connection_request_authorizations actual WHERE actual.connection_id=?1 AND actual.scale_set_id=?2 AND actual.request_id=?7 AND actual.binding_digest=?3 AND actual.acquire_state='acquired' AND actual.completed_event_digest IS NOT NULL AND actual.completed_intent_id=?4 AND actual.completed_runner_id=?5 AND actual.completed_runner_name=?6)",
            (authorization.connection_id.as_str(), authorization.scale_set_id, authorization.binding_digest.as_str(), intent_id, runner_id, runner_name, authorization.request_id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    if row.get::<i64>(0).map_err(|_| HostError::Journal)? == 1 {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}
