//! Durable intent and phase transitions for guest resource probes.

use super::Journal;
use crate::error::HostError;
mod phase;
pub(crate) use phase::ProbePhase;

/// Complete safe-to-persist identity, excluding the raw Docker root path.
#[derive(Debug, Clone)]
pub(crate) struct ProbeSeed {
    /// Random 128-bit lowercase hexadecimal operation ID.
    pub(crate) operation_id: String,
    /// Stable journal instance identity.
    pub(crate) instance_id: String,
    /// Selected Docker daemon identity.
    pub(crate) engine_id: String,
    /// Domain-separated digest of the canonical Docker root path.
    pub(crate) docker_root_digest: String,
    /// Exact source revision accepted by the image verifier.
    pub(crate) source_revision: String,
    /// Daemon-specific immutable runtime image reference.
    pub(crate) runtime_image_id: String,
    /// Digest binding source, authority, archive, platform, and store identity.
    pub(crate) image_binding_digest: String,
    /// Operation-scoped Docker name.
    pub(crate) operation_name: String,
    /// Canonical digest of the full create projection.
    pub(crate) projection_digest: String,
}

/// The only unresolved probe operation, if one is present.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProbeRow {
    /// Durable operation ID.
    pub(crate) operation_id: String,
    /// Stable journal instance identity.
    pub(crate) instance_id: String,
    /// Selected Docker daemon identity.
    pub(crate) engine_id: String,
    /// Domain-separated Docker root digest.
    pub(crate) docker_root_digest: String,
    /// Accepted probe source revision.
    pub(crate) source_revision: String,
    /// Daemon-specific immutable runtime image reference.
    pub(crate) runtime_image_id: String,
    /// Verified image binding digest.
    pub(crate) image_binding_digest: String,
    /// Exact operation-scoped Docker name.
    pub(crate) operation_name: String,
    /// Fixed controller-owned role.
    pub(crate) role: String,
    /// Canonical projection digest.
    pub(crate) projection_digest: String,
    /// Current durable external-effect phase.
    pub(crate) phase: ProbePhase,
    /// Immutable container ID, once the create response is committed.
    pub(crate) container_id: Option<String>,
}

impl Journal {
    /// Persist `Prepared` only for the currently bound journal and engine.
    pub(crate) async fn prepare_probe(&self, seed: ProbeSeed) -> Result<(), HostError> {
        if !seed_valid(&seed) {
            return Err(HostError::Journal);
        }
        let _write = self.write_guard().await;
        self.sync_lineage().await?;
        let connection = self.connection().await?;
        transaction(&connection, async {
            validate_binding(&connection, &seed).await?;
            ensure_no_active(&connection).await?;
            connection
                .execute(
                    "INSERT INTO resource_probe_operations (operation_id, instance_id, engine_id, docker_root_digest, source_revision, runtime_image_id, image_binding_digest, operation_name, role, projection_digest, phase) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'resource-probe', ?9, 'prepared')",
                    (seed.operation_id, seed.instance_id, seed.engine_id, seed.docker_root_digest, seed.source_revision, seed.runtime_image_id, seed.image_binding_digest, seed.operation_name, seed.projection_digest),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            Ok(())
        })
        .await?;
        self.sync_lineage().await
    }

    /// Read the single unresolved operation without returning root paths or output.
    pub(crate) async fn active_probe(&self) -> Result<Option<ProbeRow>, HostError> {
        let connection = self.connection().await?;
        let mut rows = connection
            .query(
                "SELECT operation_id, instance_id, engine_id, docker_root_digest, source_revision, runtime_image_id, image_binding_digest, operation_name, role, projection_digest, phase, container_id FROM resource_probe_operations WHERE phase NOT IN ('removed', 'aborted') LIMIT 2",
                (),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        let Some(first) = rows.next().await.map_err(|_| HostError::Journal)? else {
            return Ok(None);
        };
        let row = decode_row(&first)?;
        if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
            return Err(HostError::Journal);
        }
        Ok(Some(row))
    }

    /// Commit one legal phase transition before its external effect.
    pub(crate) async fn transition_probe(
        &self,
        operation_id: &str,
        expected: ProbePhase,
        next: ProbePhase,
    ) -> Result<(), HostError> {
        if !expected.allows(next) || !hex_string(operation_id, 32) {
            return Err(HostError::Journal);
        }
        self.update_probe_phase(operation_id, expected, next).await
    }

    /// Bind exactly one immutable container ID after create or exact recovery.
    pub(crate) async fn bind_probe_container(
        &self,
        operation_id: &str,
        container_id: &str,
    ) -> Result<(), HostError> {
        if !hex_string(operation_id, 32) || !container_id_valid(container_id) {
            return Err(HostError::Journal);
        }
        let _write = self.write_guard().await;
        self.sync_lineage().await?;
        let connection = self.connection().await?;
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
        self.sync_lineage().await
    }

    /// Safely retire a pre-effect row; no Docker call is permitted by this API.
    pub(crate) async fn abort_prepared_probe(&self, operation_id: &str) -> Result<(), HostError> {
        self.update_probe_phase(operation_id, ProbePhase::Prepared, ProbePhase::Aborted)
            .await
    }

    /// Commit exact same-engine absence after the controller has inspected both references.
    pub(crate) async fn confirm_probe_absent(
        &self,
        operation_id: &str,
        expected: ProbePhase,
    ) -> Result<(), HostError> {
        if matches!(
            expected,
            ProbePhase::Prepared
                | ProbePhase::Removed
                | ProbePhase::Aborted
                | ProbePhase::Quarantined
        ) || !hex_string(operation_id, 32)
        {
            return Err(HostError::Journal);
        }
        let _write = self.write_guard().await;
        self.sync_lineage().await?;
        let connection = self.connection().await?;
        let changed = connection
            .execute(
                "UPDATE resource_probe_operations SET phase = 'removed' WHERE operation_id = ?1 AND phase = ?2",
                (operation_id.to_owned(), expected.as_str().to_owned()),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        if changed != 1 {
            return Err(HostError::Journal);
        }
        self.sync_lineage().await
    }

    /// Mark ownership or execution ambiguity without deleting or freeing it.
    pub(crate) async fn quarantine_probe(&self, operation_id: &str) -> Result<(), HostError> {
        let row = self
            .active_probe()
            .await?
            .filter(|row| row.operation_id == operation_id)
            .ok_or(HostError::Journal)?;
        self.update_probe_phase(operation_id, row.phase, ProbePhase::Quarantined)
            .await
    }

    async fn update_probe_phase(
        &self,
        operation_id: &str,
        expected: ProbePhase,
        next: ProbePhase,
    ) -> Result<(), HostError> {
        if !expected.allows(next) || !hex_string(operation_id, 32) {
            return Err(HostError::Journal);
        }
        let _write = self.write_guard().await;
        self.sync_lineage().await?;
        let connection = self.connection().await?;
        let changed = connection
            .execute(
                "UPDATE resource_probe_operations SET phase = ?1 WHERE operation_id = ?2 AND phase = ?3",
                (next.as_str().to_owned(), operation_id.to_owned(), expected.as_str().to_owned()),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        if changed != 1 {
            return Err(HostError::Journal);
        }
        self.sync_lineage().await
    }
}

async fn validate_binding(
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

async fn ensure_no_active(connection: &turso::Connection) -> Result<(), HostError> {
    let mut rows = connection
        .query(
            "SELECT operation_id FROM resource_probe_operations WHERE phase NOT IN ('removed', 'aborted') LIMIT 1",
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

async fn transaction<T>(
    connection: &turso::Connection,
    operation: impl std::future::Future<Output = Result<T, HostError>>,
) -> Result<T, HostError> {
    connection
        .execute("BEGIN IMMEDIATE", ())
        .await
        .map_err(|_| HostError::Journal)?;
    let result = operation.await;
    let ended = if result.is_ok() {
        connection.execute("COMMIT", ()).await
    } else {
        connection.execute("ROLLBACK", ()).await
    };
    ended.map_err(|_| HostError::Journal)?;
    result
}

fn decode_row(row: &turso::Row) -> Result<ProbeRow, HostError> {
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
    })
}

fn seed_valid(seed: &ProbeSeed) -> bool {
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
