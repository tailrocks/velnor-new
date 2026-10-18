//! Durable host-global image import intents.

use crate::error::HostError;

use super::read_table_columns;

pub(super) async fn bootstrap(conn: &turso::Connection) -> Result<(), HostError> {
    for statement in [
        "CREATE TABLE IF NOT EXISTS image_imports (id INTEGER PRIMARY KEY AUTOINCREMENT, engine_id TEXT NOT NULL, docker_root_digest TEXT NOT NULL CHECK (length(docker_root_digest) = 64 AND docker_root_digest NOT GLOB '*[^0-9a-f]*'), daemon_architecture TEXT NOT NULL CHECK (daemon_architecture IN ('x86_64', 'aarch64')), source_sha TEXT NOT NULL CHECK (length(source_sha) = 40 AND source_sha NOT GLOB '*[^0-9a-f]*'), archive_sha256 TEXT NOT NULL CHECK (length(archive_sha256) = 64 AND archive_sha256 NOT GLOB '*[^0-9a-f]*'), index_digest TEXT NOT NULL CHECK (length(index_digest) = 64 AND index_digest NOT GLOB '*[^0-9a-f]*'), platform TEXT NOT NULL CHECK (platform = 'linux/amd64'), manifest_digest TEXT NOT NULL CHECK (length(manifest_digest) = 71 AND substr(manifest_digest, 1, 7) = 'sha256:' AND substr(manifest_digest, 8) NOT GLOB '*[^0-9a-f]*'), config_digest TEXT NOT NULL CHECK (length(config_digest) = 71 AND substr(config_digest, 1, 7) = 'sha256:' AND substr(config_digest, 8) NOT GLOB '*[^0-9a-f]*'), attempt INTEGER NOT NULL CHECK (attempt >= 0), state TEXT NOT NULL CHECK (state IN ('prepared', 'requested', 'uncertain', 'completed', 'aborted')), image_id TEXT, CHECK ((state = 'completed' AND image_id IS NOT NULL) OR (state != 'completed' AND image_id IS NULL)), UNIQUE (engine_id, docker_root_digest, daemon_architecture, source_sha, archive_sha256, index_digest, platform, manifest_digest, config_digest, attempt))",
        "CREATE INDEX IF NOT EXISTS image_imports_engine_state ON image_imports(engine_id, state, id)",
    ] {
        conn.execute(statement, ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    validate(conn).await
}

pub(super) async fn validate(conn: &turso::Connection) -> Result<(), HostError> {
    let expected = [
        ("id", "INTEGER", false, 1),
        ("engine_id", "TEXT", true, 0),
        ("docker_root_digest", "TEXT", true, 0),
        ("daemon_architecture", "TEXT", true, 0),
        ("source_sha", "TEXT", true, 0),
        ("archive_sha256", "TEXT", true, 0),
        ("index_digest", "TEXT", true, 0),
        ("platform", "TEXT", true, 0),
        ("manifest_digest", "TEXT", true, 0),
        ("config_digest", "TEXT", true, 0),
        ("attempt", "INTEGER", true, 0),
        ("state", "TEXT", true, 0),
        ("image_id", "TEXT", false, 0),
    ];
    let columns = read_table_columns(conn, "image_imports").await?;
    if columns.len() != expected.len() {
        return Err(HostError::Journal);
    }
    for (name, kind, not_null, primary_key) in expected {
        let Some(column) = columns.get(name) else {
            return Err(HostError::Journal);
        };
        if !column.declared_type.eq_ignore_ascii_case(kind)
            || column.not_null != not_null
            || column.primary_key_position != primary_key
            || column.default_value.is_some()
        {
            return Err(HostError::Journal);
        }
    }
    validate_table_sql(conn).await?;
    validate_identity_index(conn).await
}

async fn validate_table_sql(conn: &turso::Connection) -> Result<(), HostError> {
    let mut rows = conn
        .query(
            "SELECT sql FROM sqlite_schema WHERE type = 'table' AND name = 'image_imports'",
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
        .to_ascii_lowercase()
        .replace(char::is_whitespace, "");
    for fragment in [
        "idintegerprimarykeyautoincrement",
        "check(length(docker_root_digest)=64anddocker_root_digestnotglob'*[^0-9a-f]*')",
        "check(daemon_architecturein('x86_64','aarch64'))",
        "check(length(source_sha)=40andsource_shanotglob'*[^0-9a-f]*')",
        "check(length(archive_sha256)=64andarchive_sha256notglob'*[^0-9a-f]*')",
        "check(length(index_digest)=64andindex_digestnotglob'*[^0-9a-f]*')",
        "check(platform='linux/amd64')",
        "check(length(manifest_digest)=71andsubstr(manifest_digest,1,7)='sha256:'andsubstr(manifest_digest,8)notglob'*[^0-9a-f]*')",
        "check(length(config_digest)=71andsubstr(config_digest,1,7)='sha256:'andsubstr(config_digest,8)notglob'*[^0-9a-f]*')",
        "check(attempt>=0)",
        "check(statein('prepared','requested','uncertain','completed','aborted'))",
        "check((state='completed'andimage_idisnotnull)or(state!='completed'andimage_idisnull))",
        "unique(engine_id,docker_root_digest,daemon_architecture,source_sha,archive_sha256,index_digest,platform,manifest_digest,config_digest,attempt)",
    ] {
        if !sql.contains(fragment) {
            return Err(HostError::Journal);
        }
    }
    Ok(())
}

async fn validate_identity_index(conn: &turso::Connection) -> Result<(), HostError> {
    let mut rows = conn
        .query("PRAGMA index_list(image_imports)", ())
        .await
        .map_err(|_| HostError::Journal)?;
    let mut unique_columns = false;
    let mut engine_state_index = false;
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        let name = row.get::<String>(1).map_err(|_| HostError::Journal)?;
        let unique = row.get::<i64>(2).map_err(|_| HostError::Journal)? != 0;
        if unique && name.starts_with("sqlite_autoindex_image_imports") {
            unique_columns = index_columns(conn, &name).await?
                == [
                    "engine_id",
                    "docker_root_digest",
                    "daemon_architecture",
                    "source_sha",
                    "archive_sha256",
                    "index_digest",
                    "platform",
                    "manifest_digest",
                    "config_digest",
                    "attempt",
                ];
        }
        if name == "image_imports_engine_state" {
            engine_state_index = index_columns(conn, &name).await? == ["engine_id", "state", "id"];
        }
    }
    if unique_columns && engine_state_index {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}

async fn index_columns(conn: &turso::Connection, index: &str) -> Result<Vec<String>, HostError> {
    let mut rows = conn
        .query(&format!("PRAGMA index_info({index})"), ())
        .await
        .map_err(|_| HostError::Journal)?;
    let mut columns = Vec::new();
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        columns.push(row.get(2).map_err(|_| HostError::Journal)?);
    }
    Ok(columns)
}
