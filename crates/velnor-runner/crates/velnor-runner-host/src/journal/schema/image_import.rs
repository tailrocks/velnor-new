//! Durable host-global image import intents.

use crate::error::HostError;
use crate::journal::{
    ImageImportIdentity, ImageImportObservation, ImageImportRequest, ImageImportState,
};

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
    validate_v5(conn).await
}

pub(super) async fn validate_v5(conn: &turso::Connection) -> Result<(), HostError> {
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
    validate_table_sql(conn, false).await?;
    validate_identity_index(conn).await
}

pub(super) async fn upgrade_identity(conn: &turso::Connection) -> Result<(), HostError> {
    validate_v5(conn).await?;
    for statement in [
        "ALTER TABLE image_imports ADD COLUMN image_index_digest TEXT CHECK (image_index_digest IS NULL OR (length(image_index_digest) = 71 AND substr(image_index_digest, 1, 7) = 'sha256:' AND substr(image_index_digest, 8) NOT GLOB '*[^0-9a-f]*'))",
        "ALTER TABLE image_imports ADD COLUMN image_index_size INTEGER CHECK (image_index_size IS NULL OR image_index_size > 0)",
        "ALTER TABLE image_imports ADD COLUMN manifest_size INTEGER CHECK (manifest_size IS NULL OR manifest_size > 0)",
        "ALTER TABLE image_imports ADD COLUMN load_archive_sha256 TEXT CHECK (load_archive_sha256 IS NULL OR (length(load_archive_sha256) = 64 AND load_archive_sha256 NOT GLOB '*[^0-9a-f]*'))",
        "ALTER TABLE image_imports ADD COLUMN load_archive_index_digest TEXT CHECK (load_archive_index_digest IS NULL OR (length(load_archive_index_digest) = 64 AND load_archive_index_digest NOT GLOB '*[^0-9a-f]*'))",
        "ALTER TABLE image_imports ADD COLUMN image_reference TEXT CHECK (image_reference IS NULL OR (length(image_reference) BETWEEN 1 AND 255 AND image_reference NOT GLOB '*[^a-z0-9:-]*'))",
        "ALTER TABLE image_imports ADD COLUMN receipt TEXT",
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
        ("image_index_digest", "TEXT", false, 0),
        ("image_index_size", "INTEGER", false, 0),
        ("manifest_size", "INTEGER", false, 0),
        ("load_archive_sha256", "TEXT", false, 0),
        ("load_archive_index_digest", "TEXT", false, 0),
        ("image_reference", "TEXT", false, 0),
        ("receipt", "TEXT", false, 0),
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
    validate_table_sql(conn, true).await?;
    validate_identity_index(conn).await?;
    validate_rows(conn).await
}

async fn validate_table_sql(conn: &turso::Connection, extended: bool) -> Result<(), HostError> {
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
    if extended {
        for fragment in [
            "check(image_index_digestisnullor(length(image_index_digest)=71andsubstr(image_index_digest,1,7)='sha256:'andsubstr(image_index_digest,8)notglob'*[^0-9a-f]*'))",
            "check(image_index_sizeisnullorimage_index_size>0)",
            "check(manifest_sizeisnullormanifest_size>0)",
            "check(load_archive_sha256isnullor(length(load_archive_sha256)=64andload_archive_sha256notglob'*[^0-9a-f]*'))",
            "check(load_archive_index_digestisnullor(length(load_archive_index_digest)=64andload_archive_index_digestnotglob'*[^0-9a-f]*'))",
            "check(image_referenceisnullor(length(image_reference)between1and255andimage_referencenotglob'*[^a-z0-9:-]*'))",
        ] {
            if !sql.contains(fragment) {
                return Err(HostError::Journal);
            }
        }
    }
    Ok(())
}

async fn validate_rows(conn: &turso::Connection) -> Result<(), HostError> {
    let mut rows = conn
        .query(
            "SELECT engine_id, docker_root_digest, daemon_architecture, source_sha, archive_sha256, index_digest, platform, manifest_digest, config_digest, image_index_digest, image_index_size, manifest_size, load_archive_sha256, load_archive_index_digest, image_reference, state, image_id, receipt FROM image_imports",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        validate_row(&row).await?;
    }
    Ok(())
}

async fn validate_row(row: &turso::Row) -> Result<(), HostError> {
    let image_index_digest: Option<String> = row.get(9).map_err(|_| HostError::Journal)?;
    let image_index_size: Option<i64> = row.get(10).map_err(|_| HostError::Journal)?;
    let manifest_size: Option<i64> = row.get(11).map_err(|_| HostError::Journal)?;
    let load_archive_sha256: Option<String> = row.get(12).map_err(|_| HostError::Journal)?;
    let load_archive_index_digest: Option<String> = row.get(13).map_err(|_| HostError::Journal)?;
    let image_reference: Option<String> = row.get(14).map_err(|_| HostError::Journal)?;
    let state = ImageImportState::parse(&row.get::<String>(15).map_err(|_| HostError::Journal)?)?;
    let image_id: Option<String> = row.get(16).map_err(|_| HostError::Journal)?;
    let receipt: Option<String> = row.get(17).map_err(|_| HostError::Journal)?;
    let legacy = image_index_digest.is_none()
        && image_index_size.is_none()
        && manifest_size.is_none()
        && load_archive_sha256.is_none()
        && load_archive_index_digest.is_none()
        && image_reference.is_none()
        && receipt.is_none();
    if legacy {
        return Ok(());
    }
    let request = ImageImportRequest::new(ImageImportIdentity {
        engine_id: row.get(0).map_err(|_| HostError::Journal)?,
        docker_root_digest: row.get(1).map_err(|_| HostError::Journal)?,
        daemon_architecture: row.get(2).map_err(|_| HostError::Journal)?,
        source_sha: row.get(3).map_err(|_| HostError::Journal)?,
        archive_sha256: row.get(4).map_err(|_| HostError::Journal)?,
        archive_index_digest: row.get(5).map_err(|_| HostError::Journal)?,
        platform: row.get(6).map_err(|_| HostError::Journal)?,
        manifest_digest: row.get(7).map_err(|_| HostError::Journal)?,
        manifest_size: manifest_size
            .and_then(|size| u64::try_from(size).ok())
            .ok_or(HostError::Journal)?,
        config_digest: row.get(8).map_err(|_| HostError::Journal)?,
        image_index_digest,
        image_index_size: image_index_size
            .map(u64::try_from)
            .transpose()
            .map_err(|_| HostError::Journal)?,
        load_archive_sha256,
        load_archive_index_digest,
        image_reference: image_reference.ok_or(HostError::Journal)?,
    })?;
    match (state, image_id, receipt) {
        (ImageImportState::Completed, Some(image_id), Some(receipt)) => {
            let observation = ImageImportObservation::from_receipt(&receipt)?;
            observation.validate_for(&request)?;
            if observation.image_id() != image_id {
                return Err(HostError::Journal);
            }
        }
        (ImageImportState::Completed, _, _) => return Err(HostError::Journal),
        (_, None, None) => {}
        _ => return Err(HostError::Journal),
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
