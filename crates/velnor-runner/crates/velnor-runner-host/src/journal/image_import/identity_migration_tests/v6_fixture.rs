//! Test fixture for database version 6.
//!
//! Source: `154c4e58d`.
//! See `v6_sources.sha256` for source information.

use std::path::Path;

use crate::error::HostError;

const FIXTURE_SQL: &str = include_str!("v6_fixture.sql");

pub(super) async fn seed(path: &Path) -> Result<(), HostError> {
    let path = path.to_str().ok_or(HostError::Path)?;
    let database = turso::Builder::new_local(path)
        .build()
        .await
        .map_err(|_| HostError::Journal)?;
    let connection = database.connect().map_err(|_| HostError::Journal)?;
    for statement in FIXTURE_SQL.split(";\n").map(str::trim) {
        if !statement.is_empty() {
            connection
                .execute(statement, ())
                .await
                .map_err(|_| HostError::Journal)?;
        }
    }
    connection
        .execute(
            "UPDATE journal_meta SET engine_id = ?1 WHERE singleton = 1 AND engine_id IS NULL",
            ["engine-a"],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "INSERT INTO image_imports (engine_id, docker_root_digest, daemon_architecture, source_sha, archive_sha256, index_digest, platform, manifest_digest, config_digest, attempt, state, image_id) VALUES ('engine-a', ?1, 'x86_64', ?2, ?3, ?4, 'linux/amd64', ?5, ?6, 0, 'completed', ?7)",
            (
                "b".repeat(64),
                "a".repeat(40),
                "c".repeat(64),
                "d".repeat(64),
                format!("sha256:{}", "e".repeat(64)),
                format!("sha256:{}", "f".repeat(64)),
                format!("sha256:{}", "f".repeat(64)),
            ),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}
