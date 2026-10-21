use crate::error::HostError;
use crate::journal::connection::ConnectionJournal;
use crate::journal::connection::storage::in_transaction;

use super::identity::ClaimIdentity;

#[cfg(test)]
#[path = "claim/persisted_test_claim.rs"]
mod persisted_test_claim;

const MIN_LEASE_SECONDS: i64 = 10;
const MAX_LEASE_SECONDS: i64 = 300;
const INTERRUPTED_ORPHAN_MARKER: &str = "EXISTS(SELECT 1 FROM connection_execution_proofs proof WHERE proof.reservation_id=reservation.id AND proof.intent_id=i.id AND proof.connection_id=a.connection_id AND proof.scale_set_id=a.scale_set_id AND proof.witness_request_id=a.witness_request_id AND proof.binding_digest=a.binding_digest AND proof.engine_id=a.engine_id AND proof.docker_root_digest=a.docker_root_digest AND proof.instance_binding_digest=s.instance_binding_digest AND proof.runner_id=j.runner_id AND proof.runner_name=j.runner_name AND proof.proof_state='denied' AND proof.deny_code='interrupted_after_restart' AND proof.challenge_state='abandoned' AND proof.actual_request_id IS NULL AND proof.started_event_digest IS NULL AND proof.challenge_file_path='/opt/velnor/source-proof/control/challenge.json' AND proof.challenge_file_inode IS NOT NULL AND proof.challenge_file_bytes=length(CAST(proof.challenge_json AS BLOB)) AND proof.challenge_file_uid=0 AND proof.challenge_file_gid=0 AND proof.challenge_file_mode=292 AND proof.challenge_mount_id IS NOT NULL AND proof.challenge_device_major IS NOT NULL AND proof.challenge_device_minor IS NOT NULL AND proof.challenge_statx_device_major IS NOT NULL AND proof.challenge_statx_device_minor IS NOT NULL AND proof.challenge_fs_type_magic IS NOT NULL AND proof.challenge_filesystem_id IS NOT NULL AND proof.challenge_filesystem_instance_digest IS NOT NULL AND proof.challenge_read_only=1 AND proof.challenge_no_copy=1)";

#[cfg(test)]
pub(super) async fn persisted_claim_for_test(
    connection: &turso::Connection,
    scope: &ConnectionJournal,
    reservation_id: i64,
) -> Result<PostJitRetirementClaim, HostError> {
    persisted_test_claim::load(connection, scope, reservation_id).await
}

/// A typed identity for one retirement claim.
pub(crate) struct PostJitRetirementClaim {
    identity: ClaimIdentity,
    claim_generation: i64,
    lease_until: i64,
    snapshot_sha256: String,
}

/// An identity for one current recovering physical attempt in this connection.
pub(crate) struct PostJitRetirementCandidate {
    identity: ClaimIdentity,
    snapshot_sha256: String,
}

impl PostJitRetirementCandidate {
    pub(crate) const fn reservation_id(&self) -> i64 {
        self.identity.reservation_id
    }

    pub(crate) const fn generation(&self) -> i64 {
        self.identity.generation
    }

    #[cfg(test)]
    pub(crate) const fn intent_id(&self) -> i64 {
        self.identity.intent_id
    }
}

impl PostJitRetirementClaim {
    pub(super) fn identity(&self) -> &ClaimIdentity {
        &self.identity
    }

    pub(super) fn snapshot_sha256(&self) -> &str {
        &self.snapshot_sha256
    }

    pub(crate) const fn reservation_id(&self) -> i64 {
        self.identity.reservation_id
    }

    pub(crate) const fn generation(&self) -> i64 {
        self.identity.generation
    }

    pub(crate) const fn intent_id(&self) -> i64 {
        self.identity.intent_id
    }

    pub(crate) const fn scale_set_id(&self) -> i64 {
        self.identity.scale_set_id
    }

    pub(crate) fn engine_id(&self) -> &str {
        &self.identity.engine_id
    }

    pub(crate) fn docker_root_digest(&self) -> &str {
        &self.identity.docker_root_digest
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

#[cfg(test)]
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

pub(super) async fn recoverable_candidates(
    scope: &ConnectionJournal,
) -> Result<Vec<PostJitRetirementCandidate>, HostError> {
    crate::journal::connection::storage::read_transaction(
        scope.journal_ref(),
        async |connection| {
            scope.validate_scope(connection).await?;
            let sql = format!(
                "SELECT current.reservation_id FROM physical_attempt_current current JOIN physical_attempts a ON a.lineage_id=current.lineage_id AND a.generation=current.generation AND a.reservation_id=current.reservation_id AND a.intent_id=current.intent_id JOIN capacity_reservations reservation ON reservation.id=a.reservation_id AND reservation.connection_id=a.connection_id JOIN connection_intents i ON i.id=a.intent_id AND i.connection_id=a.connection_id JOIN capacity_reservation_provisioning_state s ON s.reservation_id=reservation.id JOIN connection_runner_jits j ON j.intent_id=i.id AND j.connection_id=i.connection_id WHERE reservation.connection_id=?1 AND reservation.binding_digest=?2 AND ((reservation.state='recovering' AND {INTERRUPTED_ORPHAN_MARKER}) OR (reservation.state IN ('reserved','uncertain','recovering') AND EXISTS(SELECT 1 FROM physical_attempt_post_jit_retirement_claims prior WHERE prior.reservation_id=reservation.id AND prior.lease_until<=unixepoch()))) AND i.kind='launch' AND NOT EXISTS(SELECT 1 FROM physical_attempt_post_jit_retirement_claims active WHERE active.reservation_id=reservation.id AND active.lease_until>unixepoch()) AND NOT EXISTS(SELECT 1 FROM physical_attempt_terminal_proofs terminal WHERE terminal.lineage_id=a.lineage_id AND terminal.generation=a.generation AND terminal.reservation_id=a.reservation_id) ORDER BY current.reservation_id"
            );
            let mut rows = connection
                .query(
                    &sql,
                    (scope.connection_id().as_str(), scope.binding_digest()),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            let mut reservation_ids = Vec::new();
            while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
                reservation_ids.push(row.get::<i64>(0).map_err(|_| HostError::Journal)?);
            }
            drop(rows);
            let mut candidates = Vec::with_capacity(reservation_ids.len());
            for reservation_id in reservation_ids {
                let Some(identity) = load_identity(connection, scope, reservation_id).await? else {
                    return Err(HostError::Identity);
                };
                candidates.push(PostJitRetirementCandidate {
                    snapshot_sha256: identity.snapshot_digest().map_err(|_| HostError::Journal)?,
                    identity,
                });
            }
            Ok(candidates)
        },
    )
    .await
}

pub(super) async fn claim_candidate(
    scope: &ConnectionJournal,
    candidate: &PostJitRetirementCandidate,
    lease_seconds: i64,
) -> Result<Option<PostJitRetirementClaim>, HostError> {
    if !(MIN_LEASE_SECONDS..=MAX_LEASE_SECONDS).contains(&lease_seconds)
        || candidate.identity.connection_id != scope.connection_id().as_str()
        || candidate.identity.binding_digest != scope.binding_digest()
    {
        return Err(HostError::Identity);
    }
    in_transaction(scope.journal_ref(), async |connection| {
        scope.validate_scope(connection).await?;
        let Some(identity) = load_identity(connection, scope, candidate.reservation_id()).await?
        else {
            return Ok(None);
        };
        if identity.snapshot_digest().map_err(|_| HostError::Journal)? != candidate.snapshot_sha256
        {
            return Ok(None);
        }
        claim_identity(connection, identity, lease_seconds).await
    })
    .await
}

#[cfg(test)]
async fn claim_in_transaction(
    connection: &turso::Connection,
    scope: &ConnectionJournal,
    reservation_id: i64,
    lease_seconds: i64,
) -> Result<Option<PostJitRetirementClaim>, HostError> {
    let Some(identity) = load_identity(connection, scope, reservation_id).await? else {
        return Ok(None);
    };
    claim_identity(connection, identity, lease_seconds).await
}

async fn claim_identity(
    connection: &turso::Connection,
    identity: ClaimIdentity,
    lease_seconds: i64,
) -> Result<Option<PostJitRetirementClaim>, HostError> {
    let Some((claim_generation, now)) = next_claim(connection, identity.reservation_id).await?
    else {
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
            &format!("SELECT a.lineage_id,a.generation,a.reservation_id,a.intent_id,a.connection_id,a.scale_set_id,a.witness_request_id,a.binding_digest,a.source_digest,auth.acquire_response_digest,a.engine_id,a.docker_root_digest,s.instance_binding_digest,s.runner_id,s.runner_name FROM physical_attempt_current cur JOIN physical_attempts a ON a.lineage_id=cur.lineage_id AND a.generation=cur.generation AND a.reservation_id=cur.reservation_id AND a.intent_id=cur.intent_id JOIN connection_request_authorizations auth ON auth.connection_id=a.connection_id AND auth.scale_set_id=a.scale_set_id AND auth.request_id=a.witness_request_id JOIN capacity_reservation_provisioning_state s ON s.reservation_id=a.reservation_id JOIN connection_runner_jits j ON j.intent_id=a.intent_id AND j.connection_id=a.connection_id JOIN connection_intents i ON i.id=a.intent_id AND i.connection_id=a.connection_id JOIN capacity_reservations reservation ON reservation.id=a.reservation_id WHERE cur.reservation_id=?1 AND a.connection_id=?2 AND a.binding_digest=?3 AND auth.binding_digest=a.binding_digest AND auth.source_digest=a.source_digest AND auth.acquire_state='acquired' AND auth.acquire_response_digest IS NOT NULL AND auth.acquire_response_json IS NOT NULL AND (a.acquire_response_digest IS NULL OR a.acquire_response_digest=auth.acquire_response_digest) AND ((reservation.state='recovering' AND {INTERRUPTED_ORPHAN_MARKER}) OR (reservation.state IN ('reserved','uncertain','recovering') AND EXISTS(SELECT 1 FROM physical_attempt_post_jit_retirement_claims prior WHERE prior.reservation_id=reservation.id AND prior.lease_until<=unixepoch()))) AND NOT EXISTS(SELECT 1 FROM physical_attempt_post_jit_retirement_claims active WHERE active.reservation_id=reservation.id AND active.lease_until>unixepoch()) AND NOT EXISTS(SELECT 1 FROM physical_attempt_terminal_proofs terminal WHERE terminal.lineage_id=a.lineage_id AND terminal.generation=a.generation AND terminal.reservation_id=a.reservation_id)"),
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
