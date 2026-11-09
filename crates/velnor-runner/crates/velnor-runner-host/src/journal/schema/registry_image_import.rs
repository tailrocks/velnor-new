// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::HostError;

const TABLE: &str = concat!(
    "CREATE TABLE image_import_registry (",
    "id INTEGER PRIMARY KEY AUTOINCREMENT, ",
    "engine_id TEXT NOT NULL CHECK (length(engine_id) BETWEEN 1 AND 128), ",
    "docker_root_digest TEXT NOT NULL CHECK (length(docker_root_digest) = 64 AND docker_root_digest NOT GLOB '*[^0-9a-f]*'), ",
    "daemon_architecture TEXT NOT NULL CHECK (daemon_architecture IN ('x86_64', 'aarch64')), ",
    "source_sha TEXT NOT NULL CHECK (length(source_sha) = 40 AND source_sha NOT GLOB '*[^0-9a-f]*'), ",
    "release_manifest_sha256 TEXT NOT NULL CHECK (length(release_manifest_sha256) = 64 AND release_manifest_sha256 NOT GLOB '*[^0-9a-f]*'), ",
    "role TEXT NOT NULL CHECK (role IN ('runner', 'dind', 'resource-probe')), ",
    "registry_digest TEXT NOT NULL CHECK (length(registry_digest) = 71 AND substr(registry_digest, 1, 7) = 'sha256:' AND substr(registry_digest, 8) NOT GLOB '*[^0-9a-f]*'), ",
    "config_digest TEXT NOT NULL CHECK (length(config_digest) = 71 AND substr(config_digest, 1, 7) = 'sha256:' AND substr(config_digest, 8) NOT GLOB '*[^0-9a-f]*' AND config_digest != registry_digest), ",
    "attempt INTEGER NOT NULL CHECK (attempt >= 0), ",
    "state TEXT NOT NULL CHECK (state IN ('prepared', 'requested', 'uncertain', 'completed', 'aborted')), ",
    "runtime_id TEXT, receipt TEXT, ",
    "CHECK ((state = 'completed' AND runtime_id IS NOT NULL AND receipt IS NOT NULL) OR (state != 'completed' AND runtime_id IS NULL AND receipt IS NULL)), ",
    "UNIQUE (engine_id, docker_root_digest, daemon_architecture, source_sha, release_manifest_sha256, role, registry_digest, config_digest, attempt))"
);

const INDEX: &str = "CREATE INDEX image_import_registry_engine_state ON image_import_registry(engine_id, state, id)";

pub(super) async fn migrate_v41_to_v42(conn: &turso::Connection) -> Result<(), HostError> {
    super::validate_persisted_schema_v41(conn).await?;
    for sql in [TABLE, INDEX] {
        conn.execute(sql, ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    super::registry_import_invariant::install(conn).await?;
    conn.execute("PRAGMA user_version = 42", ())
        .await
        .map_err(|_| HostError::Journal)?;
    super::validate_persisted_schema_v42(conn).await
}

pub(super) async fn validate(conn: &turso::Connection) -> Result<(), HostError> {
    super::registry_import_invariant::validate(conn).await?;
    for (kind, name, expected) in [
        ("table", "image_import_registry", TABLE),
        ("index", "image_import_registry_engine_state", INDEX),
    ] {
        let mut rows = conn
            .query(
                "SELECT sql FROM sqlite_schema WHERE type = ?1 AND name = ?2",
                (kind, name),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        let row = rows
            .next()
            .await
            .map_err(|_| HostError::Journal)?
            .ok_or(HostError::Journal)?;
        let actual: String = row.get(0).map_err(|_| HostError::Journal)?;
        if rows.next().await.map_err(|_| HostError::Journal)?.is_some()
            || super::sql_normalization::normalize(&actual)
                != super::sql_normalization::normalize(expected)
        {
            return Err(HostError::Journal);
        }
    }
    crate::journal::image_import::registry::validate_rows(conn).await
}
