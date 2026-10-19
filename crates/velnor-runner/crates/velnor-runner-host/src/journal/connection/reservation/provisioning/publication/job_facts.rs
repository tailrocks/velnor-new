use crate::error::HostError;
use crate::journal::connection::ConnectionJournal;
use crate::journal::connection::reservation::storage_budget::{
    BoundJobFactsSnapshot, JobFactsReadContext, read_job_facts_snapshot,
};
use crate::journal::connection::storage::in_transaction;
use crate::launch::bootstrap::JobFactsPublication;
use crate::worker::execution_authorization::VerifiedProofChannelBinding;
use crate::worker::storage_initialization::VerifiedStorageInitializationReady;

use super::super::admission::sql_i64;
use super::super::{hold_context, hold_is_claimable, mark_hold_uncertain};
use super::JobFactsPublicationClaim;
use super::validation::{
    bound_state, jit_runner, packed_device, same_runner, validate_claim_state,
    validate_publication_claim,
};

impl ConnectionJournal {
    /// Read bound storage facts for the exact publication claim.
    pub(crate) async fn job_facts_snapshot(
        &self,
        claim: &JobFactsPublicationClaim,
        ready: &VerifiedStorageInitializationReady,
        channel: &VerifiedProofChannelBinding,
    ) -> Result<BoundJobFactsSnapshot, HostError> {
        validate_publication_claim(self, claim)?;
        read_job_facts_snapshot(
            self,
            JobFactsReadContext {
                reservation_id: claim.reservation_id,
                operation_id: claim.operation_id,
                connection_id: &claim.connection_id,
                engine_id: &claim.engine_id,
                docker_root_digest: &claim.docker_root_digest,
                runner_container_id: &claim.runner_container_id,
                dind_container_id: &claim.dind_container_id,
                runner_id: claim.runner.id,
                runner_name: &claim.runner.name,
                scale_set_id: claim.runner.runner_scale_set_id,
                instance_binding_digest: &claim.instance_binding_digest,
                profile_generation_id: &claim.profile_generation_id,
                profile_digest: &claim.profile_digest,
                sample_generation_id: &claim.sample_generation_id,
            },
            ready,
            channel,
        )
        .await
    }

    /// After the journal records the exact runner JIT data, record permission to publish one job-facts file.
    pub(crate) async fn claim_job_facts_publication(
        &self,
        reservation_id: i64,
    ) -> Result<Option<JobFactsPublicationClaim>, HostError> {
        if reservation_id <= 0 {
            return Err(HostError::Identity);
        }
        let connection_id = self.connection_id().as_str().to_owned();
        in_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            let Some(context) = hold_context(connection, &connection_id, reservation_id).await? else {
                return Ok(None);
            };
            if !hold_is_claimable(&context) {
                return Ok(None);
            }
            let Some(record) = bound_state(connection, reservation_id).await? else {
                return Ok(None);
            };
            if !record.resources_bound
                || record.bootstrap_state != "started"
                || record.jit_state != "bound"
                || record.publication_state != "not_requested"
                || record.delivery_state != "not_requested"
                || record.cleanup_proven
            {
                return Ok(None);
            }
            let runner = jit_runner(connection, &connection_id, context.operation_id).await?;
            let changed = connection
                .execute(
                    "UPDATE capacity_reservation_provisioning_state SET publication_state = 'requested' WHERE reservation_id = ?1 AND resources_bound = 1 AND bootstrap_start_state = 'started' AND jit_state = 'bound' AND publication_state = 'not_requested' AND delivery_state = 'not_requested' AND storage_cleanup_proven = 0",
                    [reservation_id],
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if changed != 1 {
                return Ok(None);
            }
            Ok(Some(JobFactsPublicationClaim {
                connection_id: connection_id.clone(),
                reservation_id,
                operation_id: context.operation_id,
                engine_id: context.engine_id,
                docker_root_digest: context.docker_root_digest,
                runner_container_id: record.runner_container_id,
                dind_container_id: record.dind_container_id,
                runner,
                instance_binding_digest: record.instance_binding_digest,
                profile_generation_id: context.profile_generation_id,
                profile_digest: context.profile_digest,
                sample_generation_id: context.sample_generation_id,
            }))
        })
        .await
    }

    /// Record the exact projection mount and file identity before JIT delivery.
    pub(crate) async fn record_job_facts_published(
        &self,
        claim: &JobFactsPublicationClaim,
        proof: &JobFactsPublication,
    ) -> Result<(), HostError> {
        validate_publication_claim(self, claim)?;
        if proof.reservation_id() != claim.reservation_id
            || proof.operation_id() != claim.operation_id
            || proof.engine_id() != claim.engine_id
            || proof.docker_root_digest() != claim.docker_root_digest
            || proof.sample_generation_id() != claim.sample_generation_id
            || proof.profile_generation_id() != claim.profile_generation_id
            || proof.profile_digest() != claim.profile_digest
            || proof.runner_container_id() != claim.runner_container_id
            || proof.dind_container_id() != claim.dind_container_id
            || !same_runner(proof.runner(), &claim.runner)
            || proof.instance_binding_digest() != claim.instance_binding_digest
            || proof.path() != "/opt/velnor/job-facts/storage-cache-admission.json"
            || !proof.read_only()
        {
            return Err(HostError::Identity);
        }
        in_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            validate_claim_state(connection, claim, "requested", "bound").await?;
            let mut rows = connection
                .query(
                    "SELECT runner_id, runner_name, scale_set_id FROM connection_runner_jits WHERE intent_id = ?1 AND connection_id = ?2 AND state = 'bound'",
                    (claim.operation_id, claim.connection_id.as_str()),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            let row = rows.next().await.map_err(|_| HostError::Journal)?.ok_or(HostError::Journal)?;
            if row.get::<i64>(0).map_err(|_| HostError::Journal)? != proof.runner().id
                || row.get::<String>(1).map_err(|_| HostError::Journal)? != proof.runner().name
                || row.get::<i64>(2).map_err(|_| HostError::Journal)? != proof.runner().runner_scale_set_id
            {
                return Err(HostError::Identity);
            }
            let device = proof.directory_identity();
            let filesystem_id = format!("{:016x}", proof.filesystem_id());
            let packed_device = packed_device(device.0, device.1)?;
            connection
                .execute(
                    "INSERT INTO capacity_reservation_provisioning_publications (reservation_id, runner_id, runner_name, runner_scale_set_id, target, projection_sha256, directory_device_major, directory_device_minor, directory_inode, mount_id, v16_filesystem_id_0, v16_filesystem_id_1, filesystem_id, filesystem_instance_digest, read_only) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, NULL, NULL, ?11, ?12, 1)",
                    turso::params![claim.reservation_id, proof.runner().id, proof.runner().name.as_str(), proof.runner().runner_scale_set_id, proof.path(), proof.projection_digest(), i64::from(device.0), i64::from(device.1), sql_i64(device.2)?, sql_i64(proof.mount_id())?, filesystem_id, proof.filesystem_instance_digest()],
                )
                .await
                .map_err(|_| HostError::Journal)?;
            let changed = connection
                .execute(
                    "UPDATE capacity_reservation_provisioning_state SET publication_state = 'published', projection_sha256 = ?1, publication_directory_device = ?2, publication_directory_inode = ?3, publication_mount_id = ?4 WHERE reservation_id = ?5 AND publication_state = 'requested' AND jit_state = 'bound' AND resources_bound = 1",
                    (proof.projection_digest(), packed_device, sql_i64(device.2)?, sql_i64(proof.mount_id())?, claim.reservation_id),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if changed != 1 {
                return Err(HostError::Journal);
            }
            let changed = connection
                .execute(
                    "UPDATE connection_intents SET launch_phase = 'job_facts_published' WHERE id = ?1 AND connection_id = ?2 AND state = 'pending' AND cleanup_proven = 0 AND launch_phase = 'jit_bound'",
                    (claim.operation_id, claim.connection_id.as_str()),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if changed == 1 { Ok(()) } else { Err(HostError::Journal) }
        })
        .await
    }

    /// Keep full storage and CPU capacity after an uncertain publication.
    pub(crate) async fn mark_job_facts_publication_uncertain(
        &self,
        claim: &JobFactsPublicationClaim,
    ) -> Result<(), HostError> {
        validate_publication_claim(self, claim)?;
        in_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            let changed = connection
                .execute(
                    "UPDATE capacity_reservation_provisioning_state SET publication_state = 'uncertain' WHERE reservation_id = ?1 AND publication_state = 'requested' AND resources_bound = 1 AND jit_state = 'bound'",
                    [claim.reservation_id],
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if changed != 1 { return Err(HostError::Journal); }
            mark_hold_uncertain(connection, &claim.connection_id, claim.reservation_id, claim.operation_id).await
        })
        .await
    }
}
