use crate::error::HostError;
use crate::journal::connection::ConnectionJournal;
use crate::journal::connection::storage::in_transaction;

use super::identity::ClaimIdentity;

const MIN_LEASE_SECONDS: i64 = 10;
const MAX_LEASE_SECONDS: i64 = 300;

/// A typed identity for one retirement claim.
pub(crate) struct PostJitRetirementClaim {
    identity: ClaimIdentity,
    claim_generation: i64,
    lease_until: i64,
    snapshot_sha256: String,
}

impl PostJitRetirementClaim {
    pub(crate) const fn reservation_id(&self) -> i64 {
        self.identity.reservation_id
    }

    pub(crate) const fn generation(&self) -> i64 {
        self.identity.generation
    }

    pub(crate) const fn claim_generation(&self) -> i64 {
        self.claim_generation
    }

    pub(crate) const fn lease_until(&self) -> i64 {
        self.lease_until
    }

    pub(crate) fn runner_name(&self) -> &str {
        &self.identity.runner_name
    }

    pub(crate) const fn runner_id(&self) -> i64 {
        self.identity.runner_id
    }
}

pub(super) async fn claim(
    scope: &ConnectionJournal,
    reservation_id: i64,
    lease_seconds: i64,
) -> Result<Option<PostJitRetirementClaim>, HostError> {
    if reservation_id <= 0 || !(MIN_LEASE_SECONDS..=MAX_LEASE_SECONDS).contains(&lease_seconds) {
        return Err(HostError::Identity);
    }
    in_transaction(scope.journal_ref(), async |connection| {
        scope.validate_scope(connection).await?;
        claim_in_transaction(connection, scope, reservation_id, lease_seconds).await
    })
    .await
}

async fn claim_in_transaction(
    connection: &turso::Connection,
    scope: &ConnectionJournal,
    reservation_id: i64,
    lease_seconds: i64,
) -> Result<Option<PostJitRetirementClaim>, HostError> {
    let Some(identity) = load_identity(connection, scope, reservation_id).await? else {
        return Ok(None);
    };
    let Some((claim_generation, now)) = next_claim(connection, reservation_id).await? else {
        return Ok(None);
    };
    let lease_until = now.checked_add(lease_seconds).ok_or(HostError::Identity)?;
    let snapshot_sha256 = identity.snapshot_digest().map_err(|_| HostError::Journal)?;
    insert_claim(
        connection,
        &identity,
        claim_generation,
        lease_until,
        &snapshot_sha256,
    )
    .await?;
    Ok(Some(PostJitRetirementClaim {
        identity,
        claim_generation,
        lease_until,
        snapshot_sha256,
    }))
}

async fn load_identity(
    connection: &turso::Connection,
    scope: &ConnectionJournal,
    reservation_id: i64,
) -> Result<Option<ClaimIdentity>, HostError> {
    let mut rows = connection
        .query(
            "SELECT a.lineage_id,a.generation,a.reservation_id,a.intent_id,a.connection_id,a.scale_set_id,a.witness_request_id,a.binding_digest,a.source_digest,auth.acquire_response_digest,a.engine_id,a.docker_root_digest,s.instance_binding_digest,s.runner_id,s.runner_name FROM physical_attempt_current cur JOIN physical_attempts a ON a.lineage_id=cur.lineage_id AND a.generation=cur.generation AND a.reservation_id=cur.reservation_id AND a.intent_id=cur.intent_id JOIN connection_request_authorizations auth ON auth.connection_id=a.connection_id AND auth.scale_set_id=a.scale_set_id AND auth.request_id=a.witness_request_id JOIN capacity_reservation_provisioning_state s ON s.reservation_id=a.reservation_id WHERE cur.reservation_id=?1 AND a.connection_id=?2 AND a.binding_digest=?3",
            (reservation_id, scope.connection_id().as_str(), scope.binding_digest()),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(None);
    };
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
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        return Err(HostError::Journal);
    }
    Ok(Some(identity))
}

async fn next_claim(
    connection: &turso::Connection,
    reservation_id: i64,
) -> Result<Option<(i64, i64)>, HostError> {
    let mut rows = connection
        .query(
            "SELECT COALESCE(MAX(claim_generation),0),COALESCE(MAX(lease_until),0),unixepoch() FROM physical_attempt_post_jit_retirement_claims WHERE reservation_id=?1",
            [reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    let generation: i64 = row.get(0).map_err(|_| HostError::Journal)?;
    let prior_lease: i64 = row.get(1).map_err(|_| HostError::Journal)?;
    let now: i64 = row.get(2).map_err(|_| HostError::Journal)?;
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some()
        || generation > 0 && prior_lease > now
    {
        return Ok(None);
    }
    Ok(Some((
        generation.checked_add(1).ok_or(HostError::Journal)?,
        now,
    )))
}

async fn insert_claim(
    connection: &turso::Connection,
    identity: &ClaimIdentity,
    claim_generation: i64,
    lease_until: i64,
    snapshot_sha256: &str,
) -> Result<(), HostError> {
    connection
        .execute(
            "INSERT INTO physical_attempt_post_jit_retirement_claims (reservation_id,claim_generation,lineage_id,generation,intent_id,connection_id,scale_set_id,witness_request_id,binding_digest,source_digest,acquire_response_digest,engine_id,docker_root_digest,instance_binding_digest,runner_id,runner_name,snapshot_sha256,lease_until,created_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,unixepoch())",
            turso::params![identity.reservation_id,claim_generation,identity.lineage_id,identity.generation,identity.intent_id,identity.connection_id.as_str(),identity.scale_set_id,identity.witness_request_id,identity.binding_digest.as_str(),identity.source_digest.as_str(),identity.acquire_response_digest.as_str(),identity.engine_id.as_str(),identity.docker_root_digest.as_str(),identity.instance_binding_digest.as_str(),identity.runner_id,identity.runner_name.as_str(),snapshot_sha256,lease_until],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "INSERT INTO physical_attempt_post_jit_retirement_progress (reservation_id,claim_generation,phase_index,phase,evidence_sha256,created_at) VALUES (?1,?2,0,'claimed',?3,unixepoch())",
            (identity.reservation_id, claim_generation, snapshot_sha256),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}
