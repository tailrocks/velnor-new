//! Transactional SQL operations for image import intents.

use crate::error::HostError;

use super::{
    ImageImportId, ImageImportInspection, ImageImportRecord, ImageImportRequest, ImageImportState,
};
use crate::journal::connection::storage::in_transaction;
use crate::journal::sql::one_row;

pub(super) async fn prepare(
    journal: &crate::journal::Journal,
    request: ImageImportRequest,
) -> Result<ImageImportRecord, HostError> {
    request.validate()?;
    in_transaction(journal, async |connection| {
        verify_engine(connection, &request).await?;
        if let Some(record) = latest_for_request(connection, &request).await? {
            if record.state != ImageImportState::Aborted {
                return Ok(record);
            }
            let next_attempt = record.attempt.checked_add(1).ok_or(HostError::Journal)?;
            if has_other_unresolved(connection, &request).await? {
                return Err(HostError::Journal);
            }
            insert_prepared(connection, &request, next_attempt).await
        } else {
            if has_unresolved(connection, request.engine_id()).await? {
                return Err(HostError::Journal);
            }
            insert_prepared(connection, &request, 0).await
        }
    })
    .await
}

pub(super) async fn claim(
    journal: &crate::journal::Journal,
    id: ImageImportId,
    request: &ImageImportRequest,
) -> Result<bool, HostError> {
    request.validate()?;
    in_transaction(journal, async |connection| {
        verify_engine(connection, request).await?;
        let record = record_by_id(connection, id, request).await?;
        if record.state != ImageImportState::Prepared {
            return Ok(false);
        }
        let changed = connection
            .execute(
                "UPDATE image_imports SET state = 'requested' WHERE id = ?1 AND state = 'prepared'",
                [id.0],
            )
            .await
            .map_err(|_| HostError::Journal)?;
        one_row(changed)?;
        Ok(true)
    })
    .await
}

pub(super) async fn mark_uncertain(
    journal: &crate::journal::Journal,
    id: ImageImportId,
    request: &ImageImportRequest,
) -> Result<(), HostError> {
    request.validate()?;
    in_transaction(journal, async |connection| {
        verify_engine(connection, request).await?;
        let record = record_by_id(connection, id, request).await?;
        match record.state {
            ImageImportState::Uncertain => Ok(()),
            ImageImportState::Requested => {
                let changed = connection
                    .execute(
                        "UPDATE image_imports SET state = 'uncertain' WHERE id = ?1 AND state = 'requested'",
                        [id.0],
                    )
                    .await
                    .map_err(|_| HostError::Journal)?;
                one_row(changed)
            }
            _ => Err(HostError::Journal),
        }
    })
    .await
}

pub(super) async fn complete(
    journal: &crate::journal::Journal,
    id: ImageImportId,
    inspection: &ImageImportInspection,
) -> Result<(), HostError> {
    inspection.request.validate()?;
    in_transaction(journal, async |connection| {
        verify_engine(connection, &inspection.request).await?;
        let record = record_by_id(connection, id, &inspection.request).await?;
        if record.state == ImageImportState::Completed {
            return (record.image_id.as_deref() == Some(&inspection.image_id))
                .then_some(())
                .ok_or(HostError::Journal);
        }
        if !matches!(
            record.state,
            ImageImportState::Prepared
                | ImageImportState::Requested
                | ImageImportState::Uncertain
        ) {
            return Err(HostError::Journal);
        }
        let changed = connection
            .execute(
                "UPDATE image_imports SET state = 'completed', image_id = ?1 WHERE id = ?2 AND state = ?3",
                (inspection.image_id.as_str(), id.0, record.state.as_str()),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        one_row(changed)
    })
    .await
}

pub(super) async fn abort_before_effect(
    journal: &crate::journal::Journal,
    id: ImageImportId,
    request: &ImageImportRequest,
) -> Result<(), HostError> {
    request.validate()?;
    in_transaction(journal, async |connection| {
        verify_engine(connection, request).await?;
        let record = record_by_id(connection, id, request).await?;
        match record.state {
            ImageImportState::Aborted => Ok(()),
            ImageImportState::Prepared => {
                let changed = connection
                    .execute(
                        "UPDATE image_imports SET state = 'aborted' WHERE id = ?1 AND state = 'prepared'",
                        [id.0],
                    )
                    .await
                    .map_err(|_| HostError::Journal)?;
                one_row(changed)
            }
            _ => Err(HostError::Journal),
        }
    })
    .await
}

async fn verify_engine(
    connection: &turso::Connection,
    request: &ImageImportRequest,
) -> Result<(), HostError> {
    if crate::journal_schema::engine_id(connection).await? == request.engine_id() {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}

async fn latest_for_request(
    connection: &turso::Connection,
    request: &ImageImportRequest,
) -> Result<Option<ImageImportRecord>, HostError> {
    let mut rows = connection
        .query(
            "SELECT id, attempt, state, image_id FROM image_imports WHERE engine_id = ?1 AND docker_root_digest = ?2 AND daemon_architecture = ?3 AND source_sha = ?4 AND archive_sha256 = ?5 AND index_digest = ?6 AND platform = ?7 AND manifest_digest = ?8 AND config_digest = ?9 ORDER BY attempt DESC LIMIT 1",
            identity_values(request),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(None);
    };
    Ok(Some(parse_record(&row, request.clone())?))
}

async fn record_by_id(
    connection: &turso::Connection,
    id: ImageImportId,
    request: &ImageImportRequest,
) -> Result<ImageImportRecord, HostError> {
    let mut rows = connection
        .query(
            "SELECT id, attempt, state, image_id FROM image_imports WHERE id = ?1 AND engine_id = ?2 AND docker_root_digest = ?3 AND daemon_architecture = ?4 AND source_sha = ?5 AND archive_sha256 = ?6 AND index_digest = ?7 AND platform = ?8 AND manifest_digest = ?9 AND config_digest = ?10",
            (
                id.0,
                request.engine_id.as_str(),
                request.docker_root_digest.as_str(),
                request.daemon_architecture.as_str(),
                request.source_sha.as_str(),
                request.archive_sha256.as_str(),
                request.index_digest.as_str(),
                request.platform.as_str(),
                request.manifest_digest.as_str(),
                request.config_digest.as_str(),
            ),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    parse_record(&row, request.clone())
}

fn parse_record(
    row: &turso::Row,
    request: ImageImportRequest,
) -> Result<ImageImportRecord, HostError> {
    Ok(ImageImportRecord {
        id: ImageImportId(row.get(0).map_err(|_| HostError::Journal)?),
        attempt: row.get(1).map_err(|_| HostError::Journal)?,
        state: ImageImportState::parse(&row.get::<String>(2).map_err(|_| HostError::Journal)?)?,
        request,
        image_id: row.get(3).map_err(|_| HostError::Journal)?,
    })
}

async fn insert_prepared(
    connection: &turso::Connection,
    request: &ImageImportRequest,
    attempt: i64,
) -> Result<ImageImportRecord, HostError> {
    connection
        .execute(
            "INSERT INTO image_imports (engine_id, docker_root_digest, daemon_architecture, source_sha, archive_sha256, index_digest, platform, manifest_digest, config_digest, attempt, state, image_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 'prepared', NULL)",
            identity_values_with_attempt(request, attempt),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let id = last_insert_id(connection).await?;
    Ok(ImageImportRecord {
        id: ImageImportId(id),
        attempt,
        state: ImageImportState::Prepared,
        request: request.clone(),
        image_id: None,
    })
}

async fn last_insert_id(connection: &turso::Connection) -> Result<i64, HostError> {
    let mut rows = connection
        .query("SELECT last_insert_rowid()", ())
        .await
        .map_err(|_| HostError::Journal)?;
    rows.next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get(0)
        .map_err(|_| HostError::Journal)
}

async fn has_unresolved(
    connection: &turso::Connection,
    engine_id: &str,
) -> Result<bool, HostError> {
    let mut rows = connection
        .query(
            "SELECT EXISTS (SELECT 1 FROM image_imports WHERE engine_id = ?1 AND state IN ('prepared', 'requested', 'uncertain'))",
            [engine_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let value: i64 = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get(0)
        .map_err(|_| HostError::Journal)?;
    Ok(value != 0)
}

async fn has_other_unresolved(
    connection: &turso::Connection,
    request: &ImageImportRequest,
) -> Result<bool, HostError> {
    let mut rows = connection
        .query(
            "SELECT EXISTS (SELECT 1 FROM image_imports WHERE engine_id = ?1 AND state IN ('prepared', 'requested', 'uncertain') AND NOT (docker_root_digest = ?2 AND daemon_architecture = ?3 AND source_sha = ?4 AND archive_sha256 = ?5 AND index_digest = ?6 AND platform = ?7 AND manifest_digest = ?8 AND config_digest = ?9))",
            (
                request.engine_id.as_str(),
                request.docker_root_digest.as_str(),
                request.daemon_architecture.as_str(),
                request.source_sha.as_str(),
                request.archive_sha256.as_str(),
                request.index_digest.as_str(),
                request.platform.as_str(),
                request.manifest_digest.as_str(),
                request.config_digest.as_str(),
            ),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let value: i64 = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get(0)
        .map_err(|_| HostError::Journal)?;
    Ok(value != 0)
}

fn identity_values(
    request: &ImageImportRequest,
) -> (&str, &str, &str, &str, &str, &str, &str, &str, &str) {
    (
        request.engine_id.as_str(),
        request.docker_root_digest.as_str(),
        request.daemon_architecture.as_str(),
        request.source_sha.as_str(),
        request.archive_sha256.as_str(),
        request.index_digest.as_str(),
        request.platform.as_str(),
        request.manifest_digest.as_str(),
        request.config_digest.as_str(),
    )
}

fn identity_values_with_attempt(
    request: &ImageImportRequest,
    attempt: i64,
) -> (&str, &str, &str, &str, &str, &str, &str, &str, &str, i64) {
    let (engine_id, root, architecture, source, archive, index, platform, manifest, config) =
        identity_values(request);
    (
        engine_id,
        root,
        architecture,
        source,
        archive,
        index,
        platform,
        manifest,
        config,
        attempt,
    )
}
