//! Additive schema for controller-owned resource-probe operations.

use turso::Connection;

use crate::error::HostError;

pub(super) async fn bootstrap(connection: &Connection) -> Result<(), HostError> {
    connection
        .execute(
            "CREATE TABLE IF NOT EXISTS resource_probe_operations (operation_id TEXT PRIMARY KEY CHECK (length(operation_id) = 32 AND operation_id NOT GLOB '*[^0-9a-f]*'), instance_id TEXT NOT NULL, engine_id TEXT NOT NULL, docker_root_digest TEXT NOT NULL CHECK (length(docker_root_digest) = 64 AND docker_root_digest NOT GLOB '*[^0-9a-f]*'), source_revision TEXT NOT NULL CHECK (length(source_revision) = 40 AND source_revision NOT GLOB '*[^0-9a-f]*'), runtime_image_id TEXT NOT NULL, image_binding_digest TEXT NOT NULL CHECK (length(image_binding_digest) = 64 AND image_binding_digest NOT GLOB '*[^0-9a-f]*'), operation_name TEXT NOT NULL UNIQUE, role TEXT NOT NULL CHECK (role = 'resource-probe'), projection_digest TEXT NOT NULL CHECK (length(projection_digest) = 64 AND projection_digest NOT GLOB '*[^0-9a-f]*'), phase TEXT NOT NULL CHECK (phase IN ('prepared', 'create_requested', 'container_created', 'start_requested', 'started', 'wait_requested', 'waited', 'logs_requested', 'output_observed', 'stop_requested', 'remove_requested', 'removed', 'aborted', 'quarantined')), container_id TEXT)",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "CREATE UNIQUE INDEX IF NOT EXISTS resource_probe_one_active ON resource_probe_operations (role) WHERE phase NOT IN ('removed', 'aborted')",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    bootstrap_lease(connection).await?;
    Ok(())
}

pub(super) async fn bootstrap_lease(connection: &Connection) -> Result<(), HostError> {
    connection
        .execute(
            "CREATE TABLE IF NOT EXISTS host_operation_leases (singleton INTEGER PRIMARY KEY CHECK (singleton = 1), operation_id TEXT NOT NULL UNIQUE CHECK (length(operation_id) = 32 AND operation_id NOT GLOB '*[^0-9a-f]*'), engine_id TEXT NOT NULL CHECK (length(engine_id) BETWEEN 1 AND 256), docker_root_digest TEXT NOT NULL CHECK (length(docker_root_digest) = 64 AND docker_root_digest NOT GLOB '*[^0-9a-f]*'), operation_name TEXT NOT NULL UNIQUE CHECK (length(operation_name) BETWEEN 1 AND 255), container_id TEXT CHECK (container_id IS NULL OR (length(container_id) = 64 AND container_id NOT GLOB '*[^0-9a-f]*')), cpu_millicores INTEGER NOT NULL CHECK (cpu_millicores > 0), memory_bytes INTEGER NOT NULL CHECK (memory_bytes > 0), state TEXT NOT NULL CHECK (state IN ('reserved', 'active', 'uncertain', 'recovering', 'released')))",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

pub(super) async fn upgrade_lease(connection: &Connection) -> Result<(), HostError> {
    bootstrap_lease(connection).await?;
    let mut rows = connection
        .query(
            "SELECT operation_id, engine_id, docker_root_digest, operation_name, container_id, phase FROM resource_probe_operations WHERE phase NOT IN ('removed', 'aborted') LIMIT 2",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(());
    };
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        return Err(HostError::Journal);
    }
    let operation_id: String = row.get(0).map_err(|_| HostError::Journal)?;
    let engine_id: String = row.get(1).map_err(|_| HostError::Journal)?;
    let root_digest: String = row.get(2).map_err(|_| HostError::Journal)?;
    let operation_name: String = row.get(3).map_err(|_| HostError::Journal)?;
    let container_id: Option<String> = row.get(4).map_err(|_| HostError::Journal)?;
    let phase: String = row.get(5).map_err(|_| HostError::Journal)?;
    let state = if phase == "prepared" {
        "reserved"
    } else {
        "uncertain"
    };
    connection
        .execute(
            "INSERT INTO host_operation_leases (singleton, operation_id, engine_id, docker_root_digest, operation_name, container_id, cpu_millicores, memory_bytes, state) VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            (
                operation_id,
                engine_id,
                root_digest,
                operation_name,
                container_id,
                i64::try_from(crate::worker::PROBE_CPU_MILLICORES)
                    .map_err(|_| HostError::Journal)?,
                i64::try_from(crate::worker::PROBE_MEMORY_LIMIT_BYTES)
                    .map_err(|_| HostError::Journal)?,
                state,
            ),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "UPDATE host_capacity_policy SET operational_cpu_millicores = MAX(operational_cpu_millicores, ?1), operational_memory_bytes = MAX(operational_memory_bytes, ?2) WHERE singleton = 1",
            (
                i64::try_from(crate::worker::PROBE_CPU_MILLICORES)
                    .map_err(|_| HostError::Journal)?,
                i64::try_from(crate::worker::PROBE_MEMORY_LIMIT_BYTES)
                    .map_err(|_| HostError::Journal)?,
            ),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

pub(super) async fn validate_lease(connection: &Connection) -> Result<(), HostError> {
    let mut rows = connection
        .query(
            "SELECT sql FROM sqlite_schema WHERE type = 'table' AND name = 'host_operation_leases'",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let sql = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<String>(0)
        .map_err(|_| HostError::Journal)?
        .to_ascii_lowercase();
    for fragment in [
        "singleton integer primary key check (singleton = 1)",
        "operation_id text not null unique",
        "engine_id text not null",
        "docker_root_digest text not null",
        "container_id text",
        "cpu_millicores integer not null",
        "memory_bytes integer not null",
        "state text not null check (state in ('reserved', 'active', 'uncertain', 'recovering', 'released'))",
    ] {
        if !sql.contains(fragment) {
            return Err(HostError::Journal);
        }
    }
    let mut columns = connection
        .query("PRAGMA table_info(host_operation_leases)", ())
        .await
        .map_err(|_| HostError::Journal)?;
    let expected = [
        ("singleton", "INTEGER", 0, 1),
        ("operation_id", "TEXT", 1, 0),
        ("engine_id", "TEXT", 1, 0),
        ("docker_root_digest", "TEXT", 1, 0),
        ("operation_name", "TEXT", 1, 0),
        ("container_id", "TEXT", 0, 0),
        ("cpu_millicores", "INTEGER", 1, 0),
        ("memory_bytes", "INTEGER", 1, 0),
        ("state", "TEXT", 1, 0),
    ];
    for (name, kind, not_null, primary_key) in expected {
        let row = columns.next().await.map_err(|_| HostError::Journal)?;
        let Some(row) = row else {
            return Err(HostError::Journal);
        };
        if row.get::<String>(1).map_err(|_| HostError::Journal)? != name
            || !row
                .get::<String>(2)
                .map_err(|_| HostError::Journal)?
                .eq_ignore_ascii_case(kind)
            || row.get::<i64>(3).map_err(|_| HostError::Journal)? != not_null
            || row.get::<i64>(5).map_err(|_| HostError::Journal)? != primary_key
        {
            return Err(HostError::Journal);
        }
    }
    if columns
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .is_some()
    {
        return Err(HostError::Journal);
    }
    validate_lease_rows(connection).await
}

async fn validate_lease_rows(connection: &Connection) -> Result<(), HostError> {
    let mut rows = connection
        .query(
            "SELECT l.operation_id, l.engine_id, l.docker_root_digest, l.operation_name, l.container_id, l.cpu_millicores, l.memory_bytes, l.state, p.phase, p.engine_id, p.docker_root_digest, p.operation_name, p.container_id FROM host_operation_leases AS l LEFT JOIN resource_probe_operations AS p ON p.operation_id = l.operation_id WHERE l.singleton = 1",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(());
    };
    let state: String = row.get(7).map_err(|_| HostError::Journal)?;
    let phase: Option<String> = row.get(8).map_err(|_| HostError::Journal)?;
    let terminal = matches!(phase.as_deref(), Some("removed" | "aborted"));
    if (state == "released") != terminal
        || (state == "reserved") != (phase.as_deref() == Some("prepared"))
        || phase.is_none()
    {
        return Err(HostError::Journal);
    }
    for (lease_index, probe_index) in [(1, 9), (2, 10), (3, 11), (4, 12)] {
        let lease: Option<String> = row.get(lease_index).map_err(|_| HostError::Journal)?;
        let probe: Option<String> = row.get(probe_index).map_err(|_| HostError::Journal)?;
        if lease != probe {
            return Err(HostError::Journal);
        }
    }
    if state != "released" {
        let cpu: i64 = row.get(5).map_err(|_| HostError::Journal)?;
        let memory: i64 = row.get(6).map_err(|_| HostError::Journal)?;
        let mut policy = connection
            .query(
                "SELECT operational_cpu_millicores, operational_memory_bytes FROM host_capacity_policy WHERE singleton = 1",
                (),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        if let Some(policy) = policy.next().await.map_err(|_| HostError::Journal)? {
            let cpu_limit: i64 = policy.get(0).map_err(|_| HostError::Journal)?;
            let memory_limit: i64 = policy.get(1).map_err(|_| HostError::Journal)?;
            if cpu > cpu_limit || memory > memory_limit {
                return Err(HostError::Journal);
            }
        }
    }
    Ok(())
}
