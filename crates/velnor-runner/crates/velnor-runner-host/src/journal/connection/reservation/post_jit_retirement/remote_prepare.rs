use crate::error::HostError;
use crate::journal::connection::ConnectionJournal;
use crate::journal::connection::storage::in_transaction;

use super::PostJitRetirementClaim;

/// A private value that authorizes one remote absence observation.
pub(crate) struct PreparedRetirementRemoteAbsence {
    reservation_id: i64,
    generation: i64,
    claim_generation: i64,
    snapshot_sha256: String,
    connection_id: String,
    binding_digest: String,
    phase1_evidence_sha256: String,
    scale_set_id: i64,
    scale_set_name: String,
    scale_set_labels: Vec<String>,
    runner_group_id: i64,
    runner_id: i64,
    runner_name: String,
}

impl PreparedRetirementRemoteAbsence {
    #[cfg(test)]
    pub(crate) fn test_fixture() -> Self {
        Self {
            reservation_id: 19,
            generation: 1,
            claim_generation: 1,
            snapshot_sha256: "a".repeat(64),
            connection_id: "remote-absence-test".to_owned(),
            binding_digest: "b".repeat(64),
            phase1_evidence_sha256: "c".repeat(64),
            scale_set_id: 7,
            scale_set_name: "integration".to_owned(),
            scale_set_labels: vec!["velnor".to_owned(), "integration".to_owned()],
            runner_group_id: 4,
            runner_id: 31,
            runner_name: "runner-31".to_owned(),
        }
    }

    pub(crate) const fn reservation_id(&self) -> i64 {
        self.reservation_id
    }

    pub(crate) const fn generation(&self) -> i64 {
        self.generation
    }

    pub(crate) const fn claim_generation(&self) -> i64 {
        self.claim_generation
    }

    pub(crate) fn snapshot_sha256(&self) -> &str {
        &self.snapshot_sha256
    }

    pub(crate) fn connection_id(&self) -> &str {
        &self.connection_id
    }

    pub(crate) fn binding_digest(&self) -> &str {
        &self.binding_digest
    }

    pub(crate) fn phase1_evidence_sha256(&self) -> &str {
        &self.phase1_evidence_sha256
    }

    pub(crate) const fn scale_set_id(&self) -> i64 {
        self.scale_set_id
    }

    pub(crate) fn scale_set_name(&self) -> &str {
        &self.scale_set_name
    }

    pub(crate) fn scale_set_labels(&self) -> &[String] {
        &self.scale_set_labels
    }

    pub(crate) const fn runner_group_id(&self) -> i64 {
        self.runner_group_id
    }

    pub(crate) const fn runner_id(&self) -> i64 {
        self.runner_id
    }

    pub(crate) fn runner_name(&self) -> &str {
        &self.runner_name
    }
}

impl ConnectionJournal {
    pub(crate) async fn prepare_retirement_remote_absence(
        &self,
        claim: &PostJitRetirementClaim,
    ) -> Result<PreparedRetirementRemoteAbsence, HostError> {
        prepare(self, claim).await
    }

    pub(crate) async fn validate_retirement_claim_for_cleanup(
        &self,
        claim: &PostJitRetirementClaim,
        identity: &crate::journal::StorageCleanupIdentity,
    ) -> Result<(), HostError> {
        in_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            let claim_identity = load_claim_identity(connection, self, claim).await?;
            if claim_identity.reservation_id != identity.reservation_id()
                || claim_identity.intent_id != identity.operation_id()
                || claim_identity.engine_id != identity.engine_id()
                || claim_identity.docker_root_digest != identity.docker_root_digest()
            {
                return Err(HostError::Ownership);
            }
            Ok(())
        })
        .await
    }
}

pub(super) async fn prepare(
    scope: &ConnectionJournal,
    claim: &PostJitRetirementClaim,
) -> Result<PreparedRetirementRemoteAbsence, HostError> {
    in_transaction(scope.journal_ref(), async |connection| {
        scope.validate_scope(connection).await?;
        let identity = load_claim_identity(connection, scope, claim).await?;
        let phase1_evidence_sha256 = load_phase1(connection, claim).await?;
        let (runner_group_id, scale_set_name, scale_set_labels) =
            load_scale_set_identity(connection, scope, identity.scale_set_id).await?;
        Ok(PreparedRetirementRemoteAbsence {
            reservation_id: claim.reservation_id(),
            generation: claim.generation(),
            claim_generation: claim.claim_generation(),
            snapshot_sha256: identity.snapshot_sha256,
            connection_id: identity.connection_id,
            binding_digest: identity.binding_digest,
            phase1_evidence_sha256,
            scale_set_id: identity.scale_set_id,
            scale_set_name,
            scale_set_labels,
            runner_group_id,
            runner_id: claim.runner_id(),
            runner_name: claim.runner_name().to_owned(),
        })
    })
    .await
}

async fn load_scale_set_identity(
    connection: &turso::Connection,
    scope: &ConnectionJournal,
    scale_set_id: i64,
) -> Result<(i64, String, Vec<String>), HostError> {
    let mut rows = connection
        .query(
            "SELECT runner_group_id,remote_id,selector_name,selector_labels,state,remote_name,remote_labels,disable_update,ownership FROM connection_scale_set_intents WHERE connection_id=?1 AND binding_digest=?2",
            (scope.connection_id().as_str(), scope.binding_digest()),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Identity)?;
    let group: i64 = row.get(0).map_err(|_| HostError::Journal)?;
    let remote_id: Option<i64> = row.get(1).map_err(|_| HostError::Journal)?;
    let name: String = row.get(2).map_err(|_| HostError::Journal)?;
    let selector_json: String = row.get(3).map_err(|_| HostError::Journal)?;
    let state: String = row.get(4).map_err(|_| HostError::Journal)?;
    let remote_name: Option<String> = row.get(5).map_err(|_| HostError::Journal)?;
    let remote_labels_json: Option<String> = row.get(6).map_err(|_| HostError::Journal)?;
    let disable_update: Option<i64> = row.get(7).map_err(|_| HostError::Journal)?;
    let ownership: Option<String> = row.get(8).map_err(|_| HostError::Journal)?;
    let labels: Vec<String> =
        serde_json::from_str(&selector_json).map_err(|_| HostError::Journal)?;
    let remote_labels: Vec<String> =
        serde_json::from_str(remote_labels_json.as_deref().ok_or(HostError::Identity)?)
            .map_err(|_| HostError::Journal)?;
    let mut expected = labels.clone();
    let mut actual = remote_labels;
    expected.sort_unstable();
    actual.sort_unstable();
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some()
        || group <= 0
        || remote_id != Some(scale_set_id)
        || remote_name.as_deref() != Some(name.as_str())
        || !matches!(state.as_str(), "created" | "adopted")
        || ownership.as_deref() != Some(state.as_str())
        || disable_update != Some(1)
        || expected != actual
    {
        return Err(HostError::Identity);
    }
    Ok((group, name, labels))
}

struct ClaimIdentity {
    reservation_id: i64,
    intent_id: i64,
    connection_id: String,
    binding_digest: String,
    snapshot_sha256: String,
    scale_set_id: i64,
    engine_id: String,
    docker_root_digest: String,
}

async fn load_claim_identity(
    connection: &turso::Connection,
    scope: &ConnectionJournal,
    claim: &PostJitRetirementClaim,
) -> Result<ClaimIdentity, HostError> {
    let mut rows = connection
        .query(
            "SELECT c.reservation_id,c.intent_id,c.connection_id,c.binding_digest,c.scale_set_id,c.snapshot_sha256,c.runner_id,c.runner_name,c.lease_until,c.source_digest,a.source_digest,a.binding_digest,a.engine_id,a.docker_root_digest,c.instance_binding_digest,s.runner_id,s.runner_name,s.instance_binding_digest FROM physical_attempt_post_jit_retirement_claims c JOIN physical_attempt_current cur ON cur.lineage_id=c.lineage_id AND cur.generation=c.generation AND cur.reservation_id=c.reservation_id AND cur.intent_id=c.intent_id JOIN physical_attempts a ON a.lineage_id=c.lineage_id AND a.generation=c.generation AND a.reservation_id=c.reservation_id AND a.intent_id=c.intent_id AND c.engine_id=a.engine_id AND c.docker_root_digest=a.docker_root_digest JOIN capacity_reservation_provisioning_state s ON s.reservation_id=c.reservation_id AND c.instance_binding_digest=s.instance_binding_digest WHERE c.reservation_id=?1 AND c.claim_generation=?2 AND c.generation=?3 AND c.runner_id=?4 AND c.runner_name=?5 AND c.connection_id=?6 AND c.binding_digest=?7 AND c.lease_until>unixepoch() AND c.claim_generation=(SELECT MAX(latest.claim_generation) FROM physical_attempt_post_jit_retirement_claims latest WHERE latest.reservation_id=c.reservation_id)",
            (
                claim.reservation_id(),
                claim.claim_generation(),
                claim.generation(),
                claim.runner_id(),
                claim.runner_name(),
                scope.connection_id().as_str(),
                scope.binding_digest(),
            ),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Ownership)?;
    let identity = ClaimIdentity {
        reservation_id: row.get(0).map_err(|_| HostError::Journal)?,
        intent_id: row.get(1).map_err(|_| HostError::Journal)?,
        connection_id: row.get(2).map_err(|_| HostError::Journal)?,
        binding_digest: row.get(3).map_err(|_| HostError::Journal)?,
        scale_set_id: row.get(4).map_err(|_| HostError::Journal)?,
        snapshot_sha256: row.get(5).map_err(|_| HostError::Journal)?,
        engine_id: row.get(12).map_err(|_| HostError::Journal)?,
        docker_root_digest: row.get(13).map_err(|_| HostError::Journal)?,
    };
    let runner_id: i64 = row.get(6).map_err(|_| HostError::Journal)?;
    let runner_name: String = row.get(7).map_err(|_| HostError::Journal)?;
    let lease_until: i64 = row.get(8).map_err(|_| HostError::Journal)?;
    let claim_source: String = row.get(9).map_err(|_| HostError::Journal)?;
    let attempt_source: String = row.get(10).map_err(|_| HostError::Journal)?;
    let attempt_binding: String = row.get(11).map_err(|_| HostError::Journal)?;
    let claim_instance: String = row.get(14).map_err(|_| HostError::Journal)?;
    let state_runner: i64 = row.get(15).map_err(|_| HostError::Journal)?;
    let state_name: String = row.get(16).map_err(|_| HostError::Journal)?;
    let state_instance: String = row.get(17).map_err(|_| HostError::Journal)?;
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some()
        || identity.connection_id != scope.connection_id().as_str()
        || identity.binding_digest != scope.binding_digest()
        || runner_id != claim.runner_id()
        || runner_name != claim.runner_name()
        || lease_until != claim.lease_until()
        || claim_source != attempt_source
        || attempt_binding != identity.binding_digest
        || identity.reservation_id != claim.reservation_id()
        || identity.intent_id <= 0
        || identity.engine_id.is_empty()
        || identity.docker_root_digest.is_empty()
        || claim_instance != state_instance
        || state_runner != runner_id
        || state_name != runner_name
    {
        return Err(HostError::Ownership);
    }
    Ok(identity)
}

async fn load_phase1(
    connection: &turso::Connection,
    claim: &PostJitRetirementClaim,
) -> Result<String, HostError> {
    let mut rows = connection
        .query(
            "SELECT phase,evidence_sha256 FROM physical_attempt_post_jit_retirement_progress WHERE reservation_id=?1 AND claim_generation=?2 AND phase_index=1",
            (claim.reservation_id(), claim.claim_generation()),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Identity)?;
    let phase: String = row.get(0).map_err(|_| HostError::Journal)?;
    let evidence: String = row.get(1).map_err(|_| HostError::Journal)?;
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some()
        || phase != "runner_stopped"
        || evidence.len() != 64
        || !evidence
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(HostError::Identity);
    }
    Ok(evidence)
}
