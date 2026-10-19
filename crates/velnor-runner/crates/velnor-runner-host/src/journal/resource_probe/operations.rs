//! Transactional probe-operation API.

use crate::error::HostError;
use crate::journal::Journal;
use crate::journal::StorageMetadataSettlementClaim;
use crate::journal::connection::ConnectionJournal;
use crate::journal::connection::storage::{in_transaction, read_transaction};
use crate::launch::VerifiedProbeCleanup;

use super::{ProbeLeaseState, ProbePhase, ProbeRow, ProbeSeed, lease};

impl Journal {
    /// Record the Prepared phase and host lease before any Docker effect.
    pub(crate) async fn prepare_probe(&self, seed: ProbeSeed) -> Result<(), HostError> {
        if !seed_valid(&seed) {
            return Err(HostError::Journal);
        }
        in_transaction(self, async |connection| {
            validate_binding(connection, &seed).await?;
            ensure_no_active(connection).await?;
            lease::prepare(connection, &seed).await?;
            connection
                .execute(
                    "INSERT INTO resource_probe_operations (operation_id, instance_id, engine_id, docker_root_digest, source_revision, runtime_image_id, image_binding_digest, operation_name, role, projection_digest, phase) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'resource-probe', ?9, 'prepared')",
                    (seed.operation_id, seed.instance_id, seed.engine_id, seed.docker_root_digest, seed.source_revision, seed.runtime_image_id, seed.image_binding_digest, seed.operation_name, seed.projection_digest),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            Ok(())
        })
        .await
    }

    /// Prepare a probe under a cleaned worker reservation's metadata capacity reservation.
    pub(crate) async fn prepare_probe_for_storage_settlement(
        &self,
        scope: &ConnectionJournal,
        reservation_id: i64,
        seed: ProbeSeed,
    ) -> Result<StorageMetadataSettlementClaim, HostError> {
        if !seed_valid(&seed) || scope.journal_ref() != self {
            return Err(HostError::Journal);
        }
        in_transaction(self, async |connection| {
            scope.validate_scope(connection).await?;
            validate_binding(connection, &seed).await?;
            ensure_no_active(connection).await?;
            lease::prepare(connection, &seed).await?;
            connection
                .execute(
                    "INSERT INTO resource_probe_operations (operation_id, instance_id, engine_id, docker_root_digest, source_revision, runtime_image_id, image_binding_digest, operation_name, role, projection_digest, phase) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'resource-probe', ?9, 'prepared')",
                    (seed.operation_id.as_str(), seed.instance_id.as_str(), seed.engine_id.as_str(), seed.docker_root_digest.as_str(), seed.source_revision.as_str(), seed.runtime_image_id.as_str(), seed.image_binding_digest.as_str(), seed.operation_name.as_str(), seed.projection_digest.as_str()),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            crate::journal::connection::insert_metadata_settlement_claim(
                connection,
                scope,
                reservation_id,
                &seed,
            )
            .await
        })
        .await
    }

    /// Read the unresolved non-Doctor probe without output data or root paths.
    pub(crate) async fn active_probe(&self) -> Result<Option<ProbeRow>, HostError> {
        read_transaction(self, async |connection| {
            let mut rows = connection
                .query(
                    "SELECT p.operation_id, p.instance_id, p.engine_id, p.docker_root_digest, p.source_revision, p.runtime_image_id, p.image_binding_digest, p.operation_name, p.role, p.projection_digest, p.phase, p.container_id, l.state FROM resource_probe_operations AS p JOIN host_operation_leases AS l ON l.operation_id = p.operation_id WHERE p.phase NOT IN ('removed', 'aborted') AND l.state != 'released' AND NOT EXISTS (SELECT 1 FROM doctor_probe_requests AS request WHERE request.child_operation_id = p.operation_id) LIMIT 2",
                    (),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            let Some(first) = rows.next().await.map_err(|_| HostError::Journal)? else {
                return Ok(None);
            };
            let probe = decode_row(&first)?;
            if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
                return Err(HostError::Journal);
            }
            Ok(Some(probe))
        })
        .await
    }

    /// Commit one legal phase transition with its lease state.
    pub(crate) async fn transition_probe(
        &self,
        operation_id: &str,
        expected: ProbePhase,
        next: ProbePhase,
    ) -> Result<(), HostError> {
        if !expected.allows(next)
            || matches!(next, ProbePhase::Removed | ProbePhase::Aborted)
            || !hex_string(operation_id, 32)
        {
            return Err(HostError::Journal);
        }
        update_probe_phase(self, operation_id, expected, next).await
    }

    /// Record one permanent container ID after a returned create result or exact recovery.
    pub(crate) async fn bind_probe_container(
        &self,
        operation_id: &str,
        container_id: &str,
    ) -> Result<(), HostError> {
        if !hex_string(operation_id, 32) || !container_id_valid(container_id) {
            return Err(HostError::Journal);
        }
        in_transaction(self, async |connection| {
            let changed = connection
                .execute(
                    "UPDATE resource_probe_operations SET container_id = ?1, phase = 'container_created' WHERE operation_id = ?2 AND phase = 'create_requested' AND container_id IS NULL",
                    (container_id.to_owned(), operation_id.to_owned()),
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

    /// Retire only a `Prepared` row, before any Docker request was claimed.
    pub(crate) async fn abort_prepared_probe(&self, operation_id: &str) -> Result<(), HostError> {
        if !hex_string(operation_id, 32) {
            return Err(HostError::Journal);
        }
        in_transaction(self, async |connection| {
            let changed = connection
                .execute(
                    "UPDATE resource_probe_operations SET phase = 'aborted' WHERE operation_id = ?1 AND phase = 'prepared' AND container_id IS NULL",
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

    /// Release the probe lease only with the adapter's exact cleanup proof.
    pub(crate) async fn retire_probe(&self, proof: VerifiedProbeCleanup) -> Result<(), HostError> {
        let operation_id = proof.operation_id();
        let engine_id = proof.engine_id();
        let root_digest = proof.docker_root_digest();
        let operation_name = proof.operation_name();
        let container_id = proof.container_id();
        if proof.phase() != ProbePhase::RemoveRequested
            || !hex_string(operation_id, 32)
            || engine_id.is_empty()
            || engine_id.len() > 256
            || engine_id.chars().any(char::is_control)
            || !hex_string(root_digest, 64)
            || operation_name != format!("velnor-resource-probe-{operation_id}")
            || !container_id.is_some_and(container_id_valid)
        {
            return Err(HostError::Journal);
        }
        let container_id = container_id.ok_or(HostError::Journal)?;
        in_transaction(self, async |connection| {
            let changed = connection
                .execute(
                    "UPDATE resource_probe_operations SET phase = 'removed' WHERE operation_id = ?1 AND phase = 'remove_requested' AND engine_id = ?2 AND docker_root_digest = ?3 AND operation_name = ?4 AND container_id = ?5",
                    (operation_id, engine_id, root_digest, operation_name, container_id),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if changed != 1 {
                return Err(HostError::Journal);
            }
            lease::release(
                connection,
                operation_id,
                engine_id,
                root_digest,
                operation_name,
                container_id,
            )
            .await?;
            crate::journal::connection::record_metadata_probe_cleanup(
                connection,
                operation_id,
                engine_id,
                root_digest,
                operation_name,
                container_id,
            )
            .await
        })
        .await
    }

    /// Mark ambiguous probe ownership without freeing its host lease.
    pub(crate) async fn quarantine_probe(&self, operation_id: &str) -> Result<(), HostError> {
        let row = self
            .active_probe()
            .await?
            .filter(|row| row.operation_id == operation_id)
            .ok_or(HostError::Journal)?;
        update_probe_phase(self, operation_id, row.phase, ProbePhase::Quarantined).await
    }

    /// Record that restart reconciliation owns the existing charged operation.
    pub(crate) async fn begin_probe_recovery(&self, operation_id: &str) -> Result<(), HostError> {
        if !hex_string(operation_id, 32) {
            return Err(HostError::Journal);
        }
        in_transaction(self, async |connection| {
            let mut rows = connection
                .query(
                    "SELECT 1 FROM resource_probe_operations WHERE operation_id = ?1 AND phase NOT IN ('removed', 'aborted') AND NOT EXISTS (SELECT 1 FROM doctor_probe_requests WHERE child_operation_id = ?1 AND state IN ('queued', 'running', 'recovering', 'uncertain'))",
                    [operation_id],
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if rows.next().await.map_err(|_| HostError::Journal)?.is_none() {
                return Err(HostError::Journal);
            }
            lease::mark_recovering(connection, operation_id).await
        })
        .await
    }
}

async fn update_probe_phase(
    journal: &Journal,
    operation_id: &str,
    expected: ProbePhase,
    next: ProbePhase,
) -> Result<(), HostError> {
    if !expected.allows(next) || !hex_string(operation_id, 32) {
        return Err(HostError::Journal);
    }
    in_transaction(journal, async |connection| {
        let changed = connection
            .execute(
                "UPDATE resource_probe_operations SET phase = ?1 WHERE operation_id = ?2 AND phase = ?3",
                (next.as_str(), operation_id, expected.as_str()),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        if changed != 1 {
            return Err(HostError::Journal);
        }
        let state = lease_state_for_phase(next);
        match state {
            ProbeLeaseState::Uncertain | ProbeLeaseState::Active => {
                lease::transition(connection, operation_id, state).await
            }
            _ => Err(HostError::Journal),
        }
    })
    .await
}

fn lease_state_for_phase(phase: ProbePhase) -> ProbeLeaseState {
    match phase {
        ProbePhase::CreateRequested
        | ProbePhase::StartRequested
        | ProbePhase::WaitRequested
        | ProbePhase::LogsRequested
        | ProbePhase::StopRequested
        | ProbePhase::RemoveRequested
        | ProbePhase::Quarantined => ProbeLeaseState::Uncertain,
        ProbePhase::ContainerCreated
        | ProbePhase::Started
        | ProbePhase::Waited
        | ProbePhase::OutputObserved => ProbeLeaseState::Active,
        ProbePhase::Prepared | ProbePhase::Removed | ProbePhase::Aborted => {
            ProbeLeaseState::Released
        }
    }
}

pub(in crate::journal) async fn validate_binding(
    connection: &turso::Connection,
    seed: &ProbeSeed,
) -> Result<(), HostError> {
    if crate::journal_schema::instance_id(connection).await? != seed.instance_id
        || crate::journal_schema::engine_id(connection).await? != seed.engine_id
    {
        return Err(HostError::Journal);
    }
    Ok(())
}

pub(super) async fn ensure_no_active(connection: &turso::Connection) -> Result<(), HostError> {
    let mut rows = connection
        .query(
            "SELECT operation_id FROM resource_probe_operations WHERE phase NOT IN ('removed', 'aborted') UNION ALL SELECT CAST(request_id AS TEXT) FROM doctor_probe_requests WHERE state IN ('queued', 'running', 'recovering', 'uncertain') LIMIT 1",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        Err(HostError::Journal)
    } else {
        Ok(())
    }
}

pub(super) async fn ensure_no_active_probe(
    connection: &turso::Connection,
) -> Result<(), HostError> {
    ensure_no_active(connection).await
}

pub(in crate::journal) fn decode_row(row: &turso::Row) -> Result<ProbeRow, HostError> {
    Ok(ProbeRow {
        operation_id: row.get(0).map_err(|_| HostError::Journal)?,
        instance_id: row.get(1).map_err(|_| HostError::Journal)?,
        engine_id: row.get(2).map_err(|_| HostError::Journal)?,
        docker_root_digest: row.get(3).map_err(|_| HostError::Journal)?,
        source_revision: row.get(4).map_err(|_| HostError::Journal)?,
        runtime_image_id: row.get(5).map_err(|_| HostError::Journal)?,
        image_binding_digest: row.get(6).map_err(|_| HostError::Journal)?,
        operation_name: row.get(7).map_err(|_| HostError::Journal)?,
        role: row.get(8).map_err(|_| HostError::Journal)?,
        projection_digest: row.get(9).map_err(|_| HostError::Journal)?,
        phase: ProbePhase::parse(&row.get::<String>(10).map_err(|_| HostError::Journal)?)?,
        container_id: row.get(11).map_err(|_| HostError::Journal)?,
        lease_state: ProbeLeaseState::parse(
            &row.get::<String>(12).map_err(|_| HostError::Journal)?,
        )?,
    })
}

pub(in crate::journal) fn seed_valid(seed: &ProbeSeed) -> bool {
    hex_string(&seed.operation_id, 32)
        && hex_string(&seed.instance_id, 32)
        && !seed.engine_id.is_empty()
        && seed.engine_id.len() <= 256
        && !seed.engine_id.chars().any(char::is_control)
        && hex_string(&seed.docker_root_digest, 64)
        && hex_string(&seed.source_revision, 40)
        && seed.runtime_image_id.starts_with("sha256:")
        && hex_string(seed.runtime_image_id.trim_start_matches("sha256:"), 64)
        && hex_string(&seed.image_binding_digest, 64)
        && seed.operation_name == format!("velnor-resource-probe-{}", seed.operation_id)
        && hex_string(&seed.projection_digest, 64)
}

fn hex_string(value: &str, len: usize) -> bool {
    value.len() == len && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn container_id_valid(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}
