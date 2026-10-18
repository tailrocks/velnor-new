//! Transactional SQL operations for image import intents.

mod rows;

use crate::error::HostError;

use super::{
    ImageImportClaim, ImageImportId, ImageImportInspection, ImageImportRecord, ImageImportRequest,
    ImageImportState,
};
use crate::journal::connection::storage::in_transaction;
use crate::journal::sql::one_row;
use rows::{insert_prepared, latest_for_request, record_by_id};

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
) -> Result<Option<ImageImportClaim>, HostError> {
    request.validate()?;
    in_transaction(journal, async |connection| {
        verify_engine(connection, request).await?;
        let record = record_by_id(connection, id, request).await?;
        if record.state != ImageImportState::Prepared {
            return Ok(None);
        }
        let changed = connection
            .execute(
                "UPDATE image_imports SET state = 'requested' WHERE id = ?1 AND state = 'prepared'",
                [id.0],
            )
            .await
            .map_err(|_| HostError::Journal)?;
        one_row(changed)?;
        Ok(Some(ImageImportClaim::new(journal, id, request)))
    })
    .await
}

pub(super) async fn mark_uncertain(
    journal: &crate::journal::Journal,
    claim: ImageImportClaim,
) -> Result<(), HostError> {
    let ImageImportClaim {
        journal: claim_journal,
        id,
        request,
    } = claim;
    if journal != &claim_journal {
        return Err(HostError::Identity);
    }
    request.validate()?;
    in_transaction(journal, async |connection| {
        verify_engine(connection, &request).await?;
        let record = record_by_id(connection, id, &request).await?;
        if record.state != ImageImportState::Requested {
            return Err(HostError::Journal);
        }
        let changed = connection
            .execute(
                "UPDATE image_imports SET state = 'uncertain' WHERE id = ?1 AND state = 'requested'",
                [id.0],
            )
            .await
            .map_err(|_| HostError::Journal)?;
        one_row(changed)
    })
    .await
}

pub(super) async fn complete_claimed(
    journal: &crate::journal::Journal,
    claim: ImageImportClaim,
    inspection: &ImageImportInspection,
) -> Result<(), HostError> {
    let ImageImportClaim {
        journal: claim_journal,
        id,
        request,
    } = claim;
    if journal != &claim_journal {
        return Err(HostError::Identity);
    }
    request.validate()?;
    inspection.request.validate()?;
    if request != inspection.request {
        return Err(HostError::Identity);
    }
    let image_id = inspection.image_id().to_owned();
    let receipt = inspection.receipt()?;
    in_transaction(journal, async |connection| {
        verify_engine(connection, &request).await?;
        let record = record_by_id(connection, id, &request).await?;
        if record.state != ImageImportState::Requested {
            return Err(HostError::Journal);
        }
        let changed = connection
            .execute(
                "UPDATE image_imports SET state = 'completed', image_id = ?1, receipt = ?2 WHERE id = ?3 AND state = 'requested'",
                (image_id.as_str(), receipt.as_str(), id.0),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        one_row(changed)
    })
    .await
}

pub(super) async fn complete_preloaded(
    journal: &crate::journal::Journal,
    id: ImageImportId,
    request: &ImageImportRequest,
    inspection: &ImageImportInspection,
) -> Result<(), HostError> {
    request.validate()?;
    inspection.request.validate()?;
    if request != &inspection.request {
        return Err(HostError::Identity);
    }
    let image_id = inspection.image_id().to_owned();
    let receipt = inspection.receipt()?;
    in_transaction(journal, async |connection| {
        verify_engine(connection, request).await?;
        let record = record_by_id(connection, id, request).await?;
        if record.state != ImageImportState::Prepared {
            return Err(HostError::Journal);
        }
        let changed = connection
            .execute(
                "UPDATE image_imports SET state = 'completed', image_id = ?1, receipt = ?2 WHERE id = ?3 AND state = 'prepared'",
                (image_id.as_str(), receipt.as_str(), id.0),
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
                request.archive_index_digest.as_str(),
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
