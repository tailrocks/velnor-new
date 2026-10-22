use crate::error::HostError;
use crate::journal::connection::ConnectionJournal;
use crate::journal::connection::storage::in_transaction;
use crate::journal::resource_probe::{ProbeSeed, ResourceProbePurpose};
use crate::journal::{Journal, PreJitCapacityProbeClaim, PreJitCleanupClaim};

use super::{ensure_no_active, lease, seed_valid, validate_binding};

impl Journal {
    pub(crate) async fn prepare_probe_for_pre_jit_cleanup(
        &self,
        scope: &ConnectionJournal,
        claim: &PreJitCleanupClaim,
        seed: ProbeSeed,
    ) -> Result<PreJitCapacityProbeClaim, HostError> {
        if scope.journal_ref() != self
            || scope.connection_id().as_str() != claim.connection_id()
            || !seed_valid(&seed)
            || seed.purpose != ResourceProbePurpose::Capacity
            || seed.engine_id != claim.engine_id()
            || seed.docker_root_digest != claim.docker_root_digest()
            || seed.request_sha256 != super::capacity_request_sha256()
        {
            return Err(HostError::Identity);
        }
        if scope.pre_jit_cleanup_phase(claim).await?
            != Some(crate::launch::bootstrap::PreJitCleanupPhase::NetworkAbsent)
        {
            return Err(HostError::Identity);
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| HostError::Journal)?
            .as_secs();
        let now = i64::try_from(now).map_err(|_| HostError::Journal)?;
        in_transaction(self, async |connection| {
            scope.validate_scope(connection).await?;
            validate_binding(connection, &seed).await?;
            ensure_no_active(connection).await?;
            let (profile_generation_id, profile_digest) =
                profile_for_pre_jit_capacity_probe(connection, claim, now).await?;
            lease::prepare(connection, &seed).await?;
            connection
                .execute(
                    "INSERT INTO resource_probe_operations (operation_id, instance_id, engine_id, docker_root_digest, source_revision, runtime_image_id, image_binding_digest, operation_name, role, projection_digest, phase, helper_role, probe_purpose, request_sha256, request_context) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'resource-probe', ?9, 'prepared', 'resource-probe', 'capacity', ?10, NULL)",
                    (
                        seed.operation_id.as_str(),
                        seed.instance_id.as_str(),
                        seed.engine_id.as_str(),
                        seed.docker_root_digest.as_str(),
                        seed.source_revision.as_str(),
                        seed.runtime_image_id.as_str(),
                        seed.image_binding_digest.as_str(),
                        seed.operation_name.as_str(),
                        seed.projection_digest.as_str(),
                        seed.request_sha256.as_str(),
                    ),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            connection
                .execute(
                    "INSERT INTO capacity_reservation_pre_jit_cleanup_probes (probe_operation_id, reservation_id, claim_generation, connection_id, operation_id, engine_id, docker_root_digest, snapshot_sha256, profile_generation_id, profile_digest) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                    (
                        seed.operation_id.as_str(),
                        claim.reservation_id(),
                        claim.generation(),
                        claim.connection_id(),
                        claim.operation_id(),
                        claim.engine_id(),
                        claim.docker_root_digest(),
                        claim.snapshot_sha256(),
                        profile_generation_id.as_str(),
                        profile_digest.as_str(),
                    ),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            Ok(PreJitCapacityProbeClaim::new(
                claim.clone(),
                profile_generation_id,
                profile_digest,
                seed,
            ))
        })
        .await
    }
}

async fn profile_for_pre_jit_capacity_probe(
    connection: &turso::Connection,
    claim: &PreJitCleanupClaim,
    now: i64,
) -> Result<(String, String), HostError> {
    let mut rows = connection
        .query(
            "SELECT profile.generation_id, profile.profile_digest FROM capacity_reservation_pre_jit_cleanup_claims AS claim JOIN capacity_reservation_pre_jit_cleanup_progress AS progress ON progress.reservation_id = claim.reservation_id AND progress.connection_id = claim.connection_id AND progress.operation_id = claim.operation_id AND progress.engine_id = claim.engine_id AND progress.docker_root_digest = claim.docker_root_digest AND progress.snapshot_sha256 = claim.snapshot_sha256 AND progress.claim_generation = claim.claim_generation JOIN capacity_reservations AS reservation ON reservation.id = claim.reservation_id JOIN capacity_reservation_provisioning AS provisioning ON provisioning.reservation_id = reservation.id JOIN host_storage_profile_current AS current_profile ON current_profile.singleton = 1 JOIN host_storage_profiles AS profile ON profile.generation_id = current_profile.generation_id WHERE claim.reservation_id = ?1 AND claim.connection_id = ?2 AND claim.operation_id = ?3 AND claim.engine_id = ?4 AND claim.docker_root_digest = ?5 AND claim.snapshot_sha256 = ?6 AND claim.claim_generation = ?7 AND claim.lease_until = ?8 AND claim.lease_until > ?9 AND claim.phase = 'claimed' AND progress.phase = 'network_absent' AND reservation.connection_id = claim.connection_id AND reservation.intent_id = claim.operation_id AND reservation.state = 'uncertain' AND provisioning.operation_id = claim.operation_id AND provisioning.engine_id = claim.engine_id AND provisioning.docker_root_digest = claim.docker_root_digest AND provisioning.profile_generation_id = profile.generation_id AND provisioning.profile_digest = profile.profile_digest",
            (
                claim.reservation_id(),
                claim.connection_id(),
                claim.operation_id(),
                claim.engine_id(),
                claim.docker_root_digest(),
                claim.snapshot_sha256(),
                claim.generation(),
                claim.lease_until(),
                now,
            ),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Err(HostError::Identity);
    };
    let profile_generation_id = row.get::<String>(0).map_err(|_| HostError::Journal)?;
    let profile_digest = row.get::<String>(1).map_err(|_| HostError::Journal)?;
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        return Err(HostError::Journal);
    }
    Ok((profile_generation_id, profile_digest))
}
