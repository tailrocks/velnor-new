use crate::error::HostError;
use crate::journal::connection::ConnectionJournal;

use super::super::identity::ClaimIdentity;
use super::PostJitRetirementClaim;

pub(super) async fn load(
    connection: &turso::Connection,
    scope: &ConnectionJournal,
    reservation_id: i64,
) -> Result<PostJitRetirementClaim, HostError> {
    scope.validate_scope(connection).await?;
    let mut rows = connection
        .query(
            "SELECT c.lineage_id,c.generation,c.reservation_id,c.intent_id,c.connection_id,c.scale_set_id,c.witness_request_id,c.binding_digest,c.source_digest,c.acquire_response_digest,c.engine_id,c.docker_root_digest,c.instance_binding_digest,c.runner_id,c.runner_name,c.claim_generation,c.lease_until,c.snapshot_sha256 FROM physical_attempt_post_jit_retirement_claims c JOIN physical_attempt_current cur ON cur.lineage_id=c.lineage_id AND cur.generation=c.generation AND cur.reservation_id=c.reservation_id AND cur.intent_id=c.intent_id JOIN physical_attempts a ON a.lineage_id=c.lineage_id AND a.generation=c.generation AND a.reservation_id=c.reservation_id AND a.intent_id=c.intent_id JOIN capacity_reservations r ON r.id=c.reservation_id AND r.intent_id=c.intent_id AND r.connection_id=c.connection_id JOIN connection_intents i ON i.id=a.intent_id AND i.connection_id=a.connection_id JOIN capacity_reservation_provisioning_state s ON s.reservation_id=r.id JOIN connection_runner_jits j ON j.intent_id=i.id AND j.connection_id=i.connection_id JOIN connection_request_authorizations auth ON auth.connection_id=a.connection_id AND auth.scale_set_id=a.scale_set_id AND auth.request_id=a.witness_request_id WHERE c.reservation_id=?1 AND c.connection_id=?2 AND c.binding_digest=?3 AND c.claim_generation=(SELECT MAX(latest.claim_generation) FROM physical_attempt_post_jit_retirement_claims latest WHERE latest.reservation_id=c.reservation_id) AND c.lease_until>unixepoch() AND a.connection_id=c.connection_id AND a.scale_set_id=c.scale_set_id AND a.witness_request_id=c.witness_request_id AND a.binding_digest=c.binding_digest AND a.source_digest=c.source_digest AND (a.acquire_response_digest IS NULL OR a.acquire_response_digest=c.acquire_response_digest) AND a.engine_id=c.engine_id AND a.docker_root_digest=c.docker_root_digest AND r.state IN ('reserved','uncertain','recovering') AND r.binding_digest=c.binding_digest AND r.engine_id=c.engine_id AND r.docker_root_digest=c.docker_root_digest AND i.kind='launch' AND i.scale_set_id=c.scale_set_id AND s.instance_binding_digest=c.instance_binding_digest AND s.runner_id=c.runner_id AND s.runner_name=c.runner_name AND j.runner_id=c.runner_id AND j.runner_name=c.runner_name AND j.scale_set_id=c.scale_set_id AND j.witness_request_id=c.witness_request_id AND auth.binding_digest=c.binding_digest AND auth.source_digest=c.source_digest AND auth.acquire_state='acquired' AND auth.acquire_response_json IS NOT NULL AND auth.acquire_response_digest=c.acquire_response_digest AND NOT EXISTS(SELECT 1 FROM physical_attempt_terminal_proofs terminal WHERE terminal.lineage_id=c.lineage_id AND terminal.generation=c.generation AND terminal.reservation_id=c.reservation_id)",
            (reservation_id, scope.connection_id().as_str(), scope.binding_digest()),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    let identity = ClaimIdentity {
        lineage_id: row.get(0).map_err(|_| HostError::Journal)?,
        generation: row.get(1).map_err(|_| HostError::Journal)?,
        reservation_id: row.get(2).map_err(|_| HostError::Journal)?,
        intent_id: row.get(3).map_err(|_| HostError::Journal)?,
        connection_id: row.get(4).map_err(|_| HostError::Journal)?,
        scale_set_id: row.get(5).map_err(|_| HostError::Journal)?,
        witness_request_id: row.get(6).map_err(|_| HostError::Journal)?,
        binding_digest: row.get(7).map_err(|_| HostError::Journal)?,
        source_digest: row.get(8).map_err(|_| HostError::Journal)?,
        acquire_response_digest: row.get(9).map_err(|_| HostError::Journal)?,
        engine_id: row.get(10).map_err(|_| HostError::Journal)?,
        docker_root_digest: row.get(11).map_err(|_| HostError::Journal)?,
        instance_binding_digest: row.get(12).map_err(|_| HostError::Journal)?,
        runner_id: row.get(13).map_err(|_| HostError::Journal)?,
        runner_name: row.get(14).map_err(|_| HostError::Journal)?,
    };
    let claim_generation: i64 = row.get(15).map_err(|_| HostError::Journal)?;
    let lease_until: i64 = row.get(16).map_err(|_| HostError::Journal)?;
    let snapshot_sha256: String = row.get(17).map_err(|_| HostError::Journal)?;
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some()
        || identity.snapshot_digest().map_err(|_| HostError::Journal)? != snapshot_sha256
    {
        return Err(HostError::Journal);
    }
    Ok(PostJitRetirementClaim {
        identity,
        claim_generation,
        lease_until,
        snapshot_sha256,
    })
}
