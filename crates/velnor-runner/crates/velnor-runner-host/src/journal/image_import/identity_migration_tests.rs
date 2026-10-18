//! Historical image-import migration regressions.

use std::path::Path;

use crate::error::HostError;
use crate::journal::Journal;

use super::{Scratch, request};

#[tokio::test]
async fn v6_rows_migrate_with_nullable_extension_and_keep_history() -> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let path = scratch.file();
    create_v6_database(&path).await?;
    let journal = Journal::open(&path).await?;
    assert_migrated_image_import_history(&journal).await
}

async fn create_v6_database(path: &Path) -> Result<(), HostError> {
    let journal = Journal::open(path).await?;
    journal.bind_engine("engine-a").await?;
    drop(journal);
    let database = turso::Builder::new_local(path.to_str().ok_or(HostError::Journal)?)
        .build()
        .await
        .map_err(|_| HostError::Journal)?;
    let connection = database.connect().map_err(|_| HostError::Journal)?;
    drop_v7_auth_tables(&connection).await?;
    connection
        .execute("DROP TABLE image_imports", ())
        .await
        .map_err(|_| HostError::Journal)?;
    create_v6_image_import_table(&connection).await?;
    Ok(())
}

async fn drop_v7_auth_tables(connection: &turso::Connection) -> Result<(), HostError> {
    for table in [
        "connection_runner_jits",
        "connection_request_authorizations",
        "connection_source_authorizations",
    ] {
        connection
            .execute(&format!("DROP TABLE {table}"), ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    Ok(())
}

async fn create_v6_image_import_table(connection: &turso::Connection) -> Result<(), HostError> {
    connection
        .execute(
            "CREATE TABLE image_imports (id INTEGER PRIMARY KEY AUTOINCREMENT, engine_id TEXT NOT NULL, docker_root_digest TEXT NOT NULL CHECK (length(docker_root_digest) = 64 AND docker_root_digest NOT GLOB '*[^0-9a-f]*'), daemon_architecture TEXT NOT NULL CHECK (daemon_architecture IN ('x86_64', 'aarch64')), source_sha TEXT NOT NULL CHECK (length(source_sha) = 40 AND source_sha NOT GLOB '*[^0-9a-f]*'), archive_sha256 TEXT NOT NULL CHECK (length(archive_sha256) = 64 AND archive_sha256 NOT GLOB '*[^0-9a-f]*'), index_digest TEXT NOT NULL CHECK (length(index_digest) = 64 AND index_digest NOT GLOB '*[^0-9a-f]*'), platform TEXT NOT NULL CHECK (platform = 'linux/amd64'), manifest_digest TEXT NOT NULL CHECK (length(manifest_digest) = 71 AND substr(manifest_digest, 1, 7) = 'sha256:' AND substr(manifest_digest, 8) NOT GLOB '*[^0-9a-f]*'), config_digest TEXT NOT NULL CHECK (length(config_digest) = 71 AND substr(config_digest, 1, 7) = 'sha256:' AND substr(config_digest, 8) NOT GLOB '*[^0-9a-f]*'), attempt INTEGER NOT NULL CHECK (attempt >= 0), state TEXT NOT NULL CHECK (state IN ('prepared', 'requested', 'uncertain', 'completed', 'aborted')), image_id TEXT, CHECK ((state = 'completed' AND image_id IS NOT NULL) OR (state != 'completed' AND image_id IS NULL)), UNIQUE (engine_id, docker_root_digest, daemon_architecture, source_sha, archive_sha256, index_digest, platform, manifest_digest, config_digest, attempt))",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "CREATE INDEX image_imports_engine_state ON image_imports(engine_id, state, id)",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    insert_v6_image_import_row(connection).await?;
    connection
        .execute("PRAGMA user_version = 6", ())
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

async fn insert_v6_image_import_row(connection: &turso::Connection) -> Result<(), HostError> {
    connection
        .execute(
            "INSERT INTO image_imports (engine_id, docker_root_digest, daemon_architecture, source_sha, archive_sha256, index_digest, platform, manifest_digest, config_digest, attempt, state, image_id) VALUES ('engine-a', ?1, 'x86_64', ?2, ?3, ?4, 'linux/amd64', ?5, ?6, 0, 'completed', ?6)",
            (
                "b".repeat(64),
                "a".repeat(40),
                "c".repeat(64),
                "d".repeat(64),
                format!("sha256:{}", "e".repeat(64)),
                format!("sha256:{}", "f".repeat(64)),
            ),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

async fn assert_migrated_image_import_history(journal: &Journal) -> Result<(), HostError> {
    let mut rows = journal
        .connection()
        .await?
        .query(
            "SELECT state, image_id, image_index_digest, image_index_size, manifest_size, load_archive_sha256, load_archive_index_digest, image_reference, receipt FROM image_imports",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    assert_eq!(
        row.get::<String>(0).map_err(|_| HostError::Journal)?,
        "completed"
    );
    assert_eq!(
        row.get::<Option<String>>(1)
            .map_err(|_| HostError::Journal)?
            .as_deref(),
        Some(format!("sha256:{}", "f".repeat(64)).as_str())
    );
    for column in [2, 5, 6, 7, 8] {
        assert!(
            row.get::<Option<String>>(column)
                .map_err(|_| HostError::Journal)?
                .is_none()
        );
    }
    for column in [3, 4] {
        assert!(
            row.get::<Option<i64>>(column)
                .map_err(|_| HostError::Journal)?
                .is_none()
        );
    }
    assert_eq!(
        journal.prepare_image_import(request("engine-a")?).await,
        Err(HostError::Identity)
    );
    Ok(())
}
