//! Image-import identity and receipt row conversions.

use crate::error::HostError;
use crate::journal::sql::one_row;

use super::super::{
    ImageImportId, ImageImportObservation, ImageImportRecord, ImageImportRequest, ImageImportState,
};

type ImageImportIdentityValues<'a> = (
    &'a str,
    &'a str,
    &'a str,
    &'a str,
    &'a str,
    &'a str,
    &'a str,
    &'a str,
    &'a str,
);

type PreparedImageImportValues<'a> = (
    &'a str,
    &'a str,
    &'a str,
    &'a str,
    &'a str,
    &'a str,
    &'a str,
    &'a str,
    &'a str,
    i64,
    Option<&'a str>,
    Option<i64>,
    i64,
    Option<&'a str>,
    Option<&'a str>,
    &'a str,
);

pub(super) async fn latest_for_request(
    connection: &turso::Connection,
    request: &ImageImportRequest,
) -> Result<Option<ImageImportRecord>, HostError> {
    let mut rows = connection
        .query(
            "SELECT id, attempt, state, image_id, image_index_digest, image_index_size, manifest_size, load_archive_sha256, load_archive_index_digest, image_reference, receipt FROM image_imports WHERE engine_id = ?1 AND docker_root_digest = ?2 AND daemon_architecture = ?3 AND source_sha = ?4 AND archive_sha256 = ?5 AND index_digest = ?6 AND platform = ?7 AND manifest_digest = ?8 AND config_digest = ?9 ORDER BY attempt DESC LIMIT 1",
            identity_values(request),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(None);
    };
    Ok(Some(parse_record(&row, request)?))
}

pub(super) async fn record_by_id(
    connection: &turso::Connection,
    id: ImageImportId,
    request: &ImageImportRequest,
) -> Result<ImageImportRecord, HostError> {
    let mut rows = connection
        .query(
            "SELECT id, attempt, state, image_id, image_index_digest, image_index_size, manifest_size, load_archive_sha256, load_archive_index_digest, image_reference, receipt FROM image_imports WHERE id = ?1 AND engine_id = ?2 AND docker_root_digest = ?3 AND daemon_architecture = ?4 AND source_sha = ?5 AND archive_sha256 = ?6 AND index_digest = ?7 AND platform = ?8 AND manifest_digest = ?9 AND config_digest = ?10",
            (
                id.0,
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
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    parse_record(&row, request)
}

pub(super) async fn insert_prepared(
    connection: &turso::Connection,
    request: &ImageImportRequest,
    attempt: i64,
) -> Result<ImageImportRecord, HostError> {
    connection
        .execute(
            "INSERT INTO image_imports (engine_id, docker_root_digest, daemon_architecture, source_sha, archive_sha256, index_digest, platform, manifest_digest, config_digest, attempt, state, image_id, image_index_digest, image_index_size, manifest_size, load_archive_sha256, load_archive_index_digest, image_reference, receipt) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 'prepared', NULL, ?11, ?12, ?13, ?14, ?15, ?16, NULL)",
            identity_values_with_attempt(request, attempt)?,
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
        receipt: None,
    })
}

fn parse_record(
    row: &turso::Row,
    request: &ImageImportRequest,
) -> Result<ImageImportRecord, HostError> {
    validate_saved_identity(row, request)?;
    let state = ImageImportState::parse(&row.get::<String>(2).map_err(|_| HostError::Journal)?)?;
    let image_id: Option<String> = row.get(3).map_err(|_| HostError::Journal)?;
    let receipt_json: Option<String> = row.get(10).map_err(|_| HostError::Journal)?;
    let receipt = receipt_json
        .as_deref()
        .map(ImageImportObservation::from_receipt)
        .transpose()?;
    match (state, image_id.as_deref(), receipt.as_ref()) {
        (ImageImportState::Completed, Some(id), Some(evidence)) if id == evidence.image_id() => {
            evidence.validate_for(request)?;
        }
        (ImageImportState::Completed, _, _) => return Err(HostError::Journal),
        (_, None, None) => {}
        _ => return Err(HostError::Journal),
    }
    Ok(ImageImportRecord {
        id: ImageImportId(row.get(0).map_err(|_| HostError::Journal)?),
        attempt: row.get(1).map_err(|_| HostError::Journal)?,
        state,
        request: request.clone(),
        image_id,
        receipt,
    })
}

fn validate_saved_identity(
    row: &turso::Row,
    request: &ImageImportRequest,
) -> Result<(), HostError> {
    let saved_index_digest: Option<String> = row.get(4).map_err(|_| HostError::Journal)?;
    let saved_index_size: Option<i64> = row.get(5).map_err(|_| HostError::Journal)?;
    let saved_manifest_size: Option<i64> = row.get(6).map_err(|_| HostError::Journal)?;
    let saved_load_sha: Option<String> = row.get(7).map_err(|_| HostError::Journal)?;
    let saved_load_index: Option<String> = row.get(8).map_err(|_| HostError::Journal)?;
    let saved_reference: Option<String> = row.get(9).map_err(|_| HostError::Journal)?;
    let saved_index_size = saved_index_size
        .map(u64::try_from)
        .transpose()
        .map_err(|_| HostError::Journal)?;
    let saved_manifest_size = saved_manifest_size
        .map(u64::try_from)
        .transpose()
        .map_err(|_| HostError::Journal)?;
    if saved_index_digest.as_deref() != request.image_index_digest.as_deref()
        || saved_index_size != request.image_index_size
        || saved_manifest_size != Some(request.manifest_size)
        || saved_load_sha.as_deref() != request.load_archive_sha256.as_deref()
        || saved_load_index.as_deref() != request.load_archive_index_digest.as_deref()
        || saved_reference.as_deref() != Some(request.image_reference.as_str())
    {
        return Err(HostError::Identity);
    }
    Ok(())
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

fn identity_values(request: &ImageImportRequest) -> ImageImportIdentityValues<'_> {
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
    )
}

fn identity_values_with_attempt(
    request: &ImageImportRequest,
    attempt: i64,
) -> Result<PreparedImageImportValues<'_>, HostError> {
    let manifest_size = i64::try_from(request.manifest_size).map_err(|_| HostError::Journal)?;
    let image_index_size = request
        .image_index_size
        .map(i64::try_from)
        .transpose()
        .map_err(|_| HostError::Journal)?;
    Ok((
        request.engine_id.as_str(),
        request.docker_root_digest.as_str(),
        request.daemon_architecture.as_str(),
        request.source_sha.as_str(),
        request.archive_sha256.as_str(),
        request.archive_index_digest.as_str(),
        request.platform.as_str(),
        request.manifest_digest.as_str(),
        request.config_digest.as_str(),
        attempt,
        request.image_index_digest.as_deref(),
        image_index_size,
        manifest_size,
        request.load_archive_sha256.as_deref(),
        request.load_archive_index_digest.as_deref(),
        request.image_reference.as_str(),
    ))
}

pub(super) fn affected_one(result: u64) -> Result<(), HostError> {
    one_row(result)
}
