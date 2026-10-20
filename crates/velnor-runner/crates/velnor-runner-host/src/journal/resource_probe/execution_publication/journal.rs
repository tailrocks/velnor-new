//! Durable publication operation transitions.

use super::super::super::connection::storage::in_transaction;
use super::super::lease::ProbeLeaseState;
use super::super::{ProbePhase, lease, operations};
use super::MAX_RECOVERY_PROJECTION_BYTES;
use super::types::{ExecutionPublicationContext, ExecutionPublicationRecoveryRecord};
use super::validation::{digest_hex, lower_hex, valid_container_id};
use crate::{error::HostError, journal::Journal, launch::VerifiedExecutionPublicationCleanup};

impl Journal {
    /// Load the immutable parent and cleanup facts for one publication helper.
    pub(crate) async fn execution_publication_recovery(
        &self,
        operation_id: &str,
    ) -> Result<Option<ExecutionPublicationRecoveryRecord>, HostError> {
        if !lower_hex(operation_id, 32) {
            return Err(HostError::Journal);
        }
        in_transaction(self, async |connection| {
        let mut rows = connection.query(
                "SELECT operation.instance_id, operation.engine_id, operation.docker_root_digest, operation.source_revision, operation.runtime_image_id, operation.image_binding_digest, operation.projection_digest, publication.context_digest, publication.recovery_projection_json, publication.recovery_projection_sha256, publication.parent_kind, publication.parent_operation_id, publication.reservation_id, publication.connection_id, publication.runner_id, publication.runner_container_id, publication.dind_container_id, publication.metadata_volume_name, publication.metadata_volume_id, publication.metadata_layout_version, publication.metadata_layout_digest, publication.guest_boot_id, publication.payload_sha256 FROM resource_probe_operations AS operation JOIN execution_publication_operations AS publication ON publication.operation_id = operation.operation_id WHERE operation.operation_id = ?1 AND operation.helper_role = 'execution-publication' AND operation.operation_name = 'velnor-execution-publication-' || operation.operation_id",
                [operation_id],
            ).await.map_err(|_| HostError::Journal)?;
        let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
            return Ok(None);
        };
        let Some(recovery_projection_json) = row
            .get::<Option<String>>(8)
            .map_err(|_| HostError::Journal)?
        else {
            return Ok(None);
        };
        let Some(recovery_projection_sha256) = row
            .get::<Option<String>>(9)
            .map_err(|_| HostError::Journal)?
        else {
            return Ok(None);
        };
        let value = ExecutionPublicationRecoveryRecord {
            operation_id: operation_id.to_owned(),
            instance_id: row.get(0).map_err(|_| HostError::Journal)?,
            engine_id: row.get(1).map_err(|_| HostError::Journal)?,
            docker_root_digest: row.get(2).map_err(|_| HostError::Journal)?,
            source_revision: row.get(3).map_err(|_| HostError::Journal)?,
            runtime_image_id: row.get(4).map_err(|_| HostError::Journal)?,
            image_binding_digest: row.get(5).map_err(|_| HostError::Journal)?,
            projection_digest: row.get(6).map_err(|_| HostError::Journal)?,
            context_digest: row.get(7).map_err(|_| HostError::Journal)?,
            recovery_projection_json,
            recovery_projection_sha256,
            parent_kind: row.get(10).map_err(|_| HostError::Journal)?,
            parent_operation_id: row.get(11).map_err(|_| HostError::Journal)?,
            reservation_id: row.get(12).map_err(|_| HostError::Journal)?,
            connection_id: row.get(13).map_err(|_| HostError::Journal)?,
            runner_id: row.get(14).map_err(|_| HostError::Journal)?,
            runner_container_id: row.get(15).map_err(|_| HostError::Journal)?,
            dind_container_id: row.get(16).map_err(|_| HostError::Journal)?,
            metadata_volume_name: row.get(17).map_err(|_| HostError::Journal)?,
            metadata_volume_id: row.get(18).map_err(|_| HostError::Journal)?,
            metadata_layout_version: row.get(19).map_err(|_| HostError::Journal)?,
            metadata_layout_digest: row.get(20).map_err(|_| HostError::Journal)?,
            guest_boot_id: row.get(21).map_err(|_| HostError::Journal)?,
            payload_sha256: row.get(22).map_err(|_| HostError::Journal)?,
        };
        if value.recovery_projection_json.is_empty()
            || value.recovery_projection_json.len() > MAX_RECOVERY_PROJECTION_BYTES
            || !lower_hex(&value.recovery_projection_sha256, 64)
            || value.recovery_projection_sha256
                != digest_hex(value.recovery_projection_json.as_bytes())
            || !lower_hex(&value.context_digest, 64)
        {
            return Err(HostError::Ownership);
        }
        Ok(Some(value))
        }).await
    }

    /// Claim one publisher operation under the shared host lease.
    pub(crate) async fn prepare_execution_publication(
        &self,
        context: ExecutionPublicationContext,
    ) -> Result<(), HostError> {
        if !context.valid()
            || context.parent.engine_id != context.engine_id
            || context.parent.docker_root_digest != context.docker_root_digest
        {
            return Err(HostError::Identity);
        }
        in_transaction(self, async |connection| {
            if crate::journal_schema::instance_id(connection).await? != context.instance_id
                || crate::journal_schema::engine_id(connection).await? != context.engine_id
            {
                return Err(HostError::Identity);
            }
            operations::ensure_no_active(connection).await?;
            lease::prepare_identity(
                connection,
                &context.operation_id,
                &context.engine_id,
                &context.docker_root_digest,
                &context.operation_name,
            )
            .await?;
            connection
                .execute(
                    "INSERT INTO resource_probe_operations (operation_id, instance_id, engine_id, docker_root_digest, source_revision, runtime_image_id, image_binding_digest, operation_name, role, projection_digest, phase, helper_role, probe_purpose, request_sha256, request_context) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'resource-probe', ?9, 'prepared', 'execution-publication', 'none', NULL, NULL)",
                    turso::params![
                        context.operation_id.as_str(),
                        context.instance_id.as_str(),
                        context.engine_id.as_str(),
                        context.docker_root_digest.as_str(),
                        context.source_revision.as_str(),
                        context.runtime_image_id.as_str(),
                        context.image_binding_digest.as_str(),
                        context.operation_name.as_str(),
                        context.projection_digest.as_str(),
                    ],
                )
                .await
                .map_err(|_| HostError::Journal)?;
            let parent = &context.parent;
            connection
                .execute(
                    "INSERT INTO execution_publication_operations (operation_id, parent_kind, parent_operation_id, reservation_id, connection_id, scale_set_id, witness_request_id, actual_request_id, runner_id, runner_name, runner_container_id, dind_container_id, engine_id, docker_root_digest, metadata_volume_name, metadata_volume_id, metadata_layout_version, metadata_layout_digest, guest_boot_id, payload_sha256, claim_digest, projection_digest, context_digest, recovery_projection_json, recovery_projection_sha256) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25)",
                    turso::params![
                        context.operation_id.as_str(),
                        parent.kind.as_str(),
                        parent.operation_id,
                        parent.reservation_id,
                        parent.connection_id.as_str(),
                        parent.scale_set_id,
                        parent.witness_request_id,
                        parent.actual_request_id,
                        parent.runner_id,
                        parent.runner_name.as_str(),
                        parent.runner_container_id.as_str(),
                        parent.dind_container_id.as_str(),
                        parent.engine_id.as_str(),
                        parent.docker_root_digest.as_str(),
                        parent.metadata_volume_name.as_str(),
                        parent.metadata_volume_id.as_str(),
                        parent.metadata_layout_version,
                        parent.metadata_layout_digest.as_str(),
                        parent.guest_boot_id.as_str(),
                        parent.payload_sha256.as_str(),
                        parent.claim_digest.as_str(),
                        context.projection_digest.as_str(),
                        context.context_digest.as_str(),
                        context.recovery_projection_json.as_str(),
                        context.recovery_projection_sha256.as_str(),
                    ],
                )
                .await
                .map_err(|_| HostError::Journal)?;
            Ok(())
        })
        .await
    }

    /// Record one legal publication-helper phase transition under its role.
    pub(crate) async fn transition_execution_publication(
        &self,
        operation_id: &str,
        expected: ProbePhase,
        next: ProbePhase,
    ) -> Result<(), HostError> {
        if !expected.allows(next)
            || matches!(next, ProbePhase::Removed | ProbePhase::Aborted)
            || !lower_hex(operation_id, 32)
        {
            return Err(HostError::Journal);
        }
        in_transaction(self, async |connection| {
            verify_publication_role(connection, operation_id).await?;
            let changed = connection
                .execute(
                    "UPDATE resource_probe_operations SET phase = ?1 WHERE operation_id = ?2 AND helper_role = 'execution-publication' AND phase = ?3",
                    (next.as_str(), operation_id, expected.as_str()),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if changed != 1 {
                return Err(HostError::Journal);
            }
            let state = publication_lease_state(next)?;
            lease::transition(connection, operation_id, state).await
        })
        .await
    }

    /// Bind one create result to the exact publication operation and lease.
    pub(crate) async fn bind_execution_publication_container(
        &self,
        operation_id: &str,
        container_id: &str,
    ) -> Result<(), HostError> {
        if !lower_hex(operation_id, 32) || !valid_container_id(container_id) {
            return Err(HostError::Journal);
        }
        in_transaction(self, async |connection| {
            verify_publication_role(connection, operation_id).await?;
            let changed = connection
                .execute(
                    "UPDATE resource_probe_operations SET container_id = ?1, phase = 'container_created' WHERE operation_id = ?2 AND helper_role = 'execution-publication' AND operation_name = 'velnor-execution-publication-' || operation_id AND phase = 'create_requested' AND container_id IS NULL",
                    (container_id, operation_id),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if changed != 1 {
                return Err(HostError::Journal);
            }
            lease::bind_container(connection, operation_id, container_id).await
        })
        .await
    }

    /// Release the shared helper lease after exact publication-container removal.
    pub(crate) async fn retire_execution_publication(
        &self,
        proof: VerifiedExecutionPublicationCleanup,
    ) -> Result<(), HostError> {
        let operation_id = proof.operation_id();
        if proof.phase() != ProbePhase::RemoveRequested
            || !lower_hex(operation_id, 32)
            || proof.operation_name() != format!("velnor-execution-publication-{operation_id}")
            || !valid_container_id(proof.container_id())
        {
            return Err(HostError::Ownership);
        }
        in_transaction(self, async |connection| {
            verify_publication_role(connection, operation_id).await?;
            let changed = connection
                .execute(
                    "UPDATE resource_probe_operations SET phase = 'removed' WHERE operation_id = ?1 AND helper_role = 'execution-publication' AND phase = 'remove_requested' AND engine_id = ?2 AND docker_root_digest = ?3 AND operation_name = ?4 AND container_id = ?5",
                    (operation_id, proof.engine_id(), proof.docker_root_digest(), proof.operation_name(), proof.container_id()),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if changed != 1 {
                return Err(HostError::Ownership);
            }
            lease::release(
                connection,
                operation_id,
                proof.engine_id(),
                proof.docker_root_digest(),
                proof.operation_name(),
                proof.container_id(),
            )
            .await
        })
        .await
    }

    /// Quarantine only the exact publication operation during restart recovery.
    pub(crate) async fn quarantine_execution_publication(
        &self,
        operation_id: &str,
        expected: ProbePhase,
    ) -> Result<(), HostError> {
        self.transition_execution_publication(operation_id, expected, ProbePhase::Quarantined)
            .await
    }

    /// Abort only a prepared publication whose Docker request did not start.
    pub(crate) async fn abort_prepared_execution_publication(
        &self,
        operation_id: &str,
    ) -> Result<(), HostError> {
        if !lower_hex(operation_id, 32) {
            return Err(HostError::Journal);
        }
        in_transaction(self, async |connection| {
            verify_publication_role(connection, operation_id).await?;
            let changed = connection
                .execute(
                    "UPDATE resource_probe_operations SET phase = 'aborted' WHERE operation_id = ?1 AND helper_role = 'execution-publication' AND phase = 'prepared' AND container_id IS NULL",
                    [operation_id],
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if changed != 1 {
                return Err(HostError::Journal);
            }
            lease::release_prepared(connection, operation_id).await
        })
        .await
    }

    /// Charge an unresolved publication to restart reconciliation.
    pub(crate) async fn begin_execution_publication_recovery(
        &self,
        operation_id: &str,
    ) -> Result<ProbePhase, HostError> {
        if !lower_hex(operation_id, 32) {
            return Err(HostError::Journal);
        }
        in_transaction(self, async |connection| {
            let mut rows = connection
                .query(
                    "SELECT phase FROM resource_probe_operations WHERE operation_id = ?1 AND helper_role = 'execution-publication' AND operation_name = 'velnor-execution-publication-' || operation_id",
                    [operation_id],
                )
                .await
                .map_err(|_| HostError::Journal)?;
            let row = rows
                .next()
                .await
                .map_err(|_| HostError::Journal)?
                .ok_or(HostError::Ownership)?;
            let phase = ProbePhase::parse(
                &row.get::<String>(0).map_err(|_| HostError::Journal)?,
            )?;
            if matches!(phase, ProbePhase::Prepared | ProbePhase::Removed | ProbePhase::Aborted) {
                return Err(HostError::Ownership);
            }
            lease::mark_recovering(connection, operation_id).await?;
            Ok(phase)
        })
        .await
    }
}
async fn verify_publication_role(
    connection: &turso::Connection,
    operation_id: &str,
) -> Result<(), HostError> {
    let mut rows = connection
        .query(
            "SELECT operation_name FROM resource_probe_operations WHERE operation_id = ?1 AND helper_role = 'execution-publication'",
            [operation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Ownership)?;
    let operation_name = row.get::<String>(0).map_err(|_| HostError::Journal)?;
    if operation_name != format!("velnor-execution-publication-{operation_id}") {
        return Err(HostError::Ownership);
    }
    Ok(())
}
fn publication_lease_state(phase: ProbePhase) -> Result<ProbeLeaseState, HostError> {
    match phase {
        ProbePhase::CreateRequested
        | ProbePhase::StartRequested
        | ProbePhase::WaitRequested
        | ProbePhase::LogsRequested
        | ProbePhase::StopRequested
        | ProbePhase::RemoveRequested
        | ProbePhase::Quarantined => Ok(ProbeLeaseState::Uncertain),
        ProbePhase::ContainerCreated
        | ProbePhase::Started
        | ProbePhase::Waited
        | ProbePhase::OutputObserved => Ok(ProbeLeaseState::Active),
        ProbePhase::Prepared | ProbePhase::Removed | ProbePhase::Aborted => Err(HostError::Journal),
    }
}
