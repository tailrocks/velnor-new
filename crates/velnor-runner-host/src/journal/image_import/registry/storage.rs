// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    HostError, ImageKind, RegistryImageIdentity, RegistryImageObservation, RegistryImageRecord,
    role_text,
};

pub(super) async fn abort_unclaimed_archive_imports(
    connection: &turso::Connection,
    engine_id: &str,
) -> Result<(), HostError> {
    crate::journal::schema::image_import::validate_rows(connection).await?;
    let mut rows = connection.query(
        "SELECT EXISTS (SELECT 1 FROM image_imports WHERE engine_id=?1 AND state='prepared' AND (image_id IS NOT NULL OR receipt IS NOT NULL))",
        [engine_id],
    ).await.map_err(|_| HostError::Journal)?;
    let invalid: i64 = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get(0)
        .map_err(|_| HostError::Journal)?;
    drop(rows);
    if invalid != 0 {
        return Err(HostError::Journal);
    }
    connection.execute(
        "UPDATE image_imports SET state='aborted' WHERE engine_id=?1 AND state='prepared' AND image_id IS NULL AND receipt IS NULL",
        [engine_id],
    ).await.map_err(|_| HostError::Journal)?;
    Ok(())
}

pub(in crate::journal) async fn validate_rows(
    connection: &turso::Connection,
) -> Result<(), HostError> {
    let mut rows = connection.query("SELECT id,engine_id,docker_root_digest,daemon_architecture,source_sha,release_manifest_sha256,role,registry_digest,config_digest,attempt,state,runtime_id,receipt FROM image_import_registry", ()).await.map_err(|_| HostError::Journal)?;
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        let id: i64 = row.get(0).map_err(|_| HostError::Journal)?;
        let identity = RegistryImageIdentity {
            engine_id: row.get(1).map_err(|_| HostError::Journal)?,
            docker_root_digest: row.get(2).map_err(|_| HostError::Journal)?,
            daemon_architecture: row.get(3).map_err(|_| HostError::Journal)?,
            source_sha: row.get(4).map_err(|_| HostError::Journal)?,
            release_manifest_sha256: row.get(5).map_err(|_| HostError::Journal)?,
            role: parse_role(&row.get::<String>(6).map_err(|_| HostError::Journal)?)?,
            registry_digest: row.get(7).map_err(|_| HostError::Journal)?,
            config_digest: row.get(8).map_err(|_| HostError::Journal)?,
        };
        identity.validate().map_err(|_| HostError::Journal)?;
        let attempt: i64 = row.get(9).map_err(|_| HostError::Journal)?;
        let state = super::super::ImageImportState::parse(
            &row.get::<String>(10).map_err(|_| HostError::Journal)?,
        )?;
        let runtime_id: Option<String> = row.get(11).map_err(|_| HostError::Journal)?;
        let receipt: Option<String> = row.get(12).map_err(|_| HostError::Journal)?;
        if id <= 0 || attempt < 0 {
            return Err(HostError::Journal);
        }
        match (state, runtime_id, receipt) {
            (super::super::ImageImportState::Completed, Some(runtime), Some(receipt)) => {
                let observation = RegistryImageObservation::from_receipt(&receipt, &identity)?;
                if observation.runtime_id() != runtime {
                    return Err(HostError::Journal);
                }
            }
            (super::super::ImageImportState::Completed, _, _) => return Err(HostError::Journal),
            (_, None, None) => {}
            _ => return Err(HostError::Journal),
        }
    }
    drop(rows);
    let mut conflicts = connection.query(
        "SELECT EXISTS (SELECT engine_id FROM (SELECT engine_id FROM image_imports WHERE state IN ('prepared','requested','uncertain') UNION ALL SELECT engine_id FROM image_import_registry WHERE state IN ('prepared','requested','uncertain')) GROUP BY engine_id HAVING COUNT(*) > 1)",
        (),
    ).await.map_err(|_| HostError::Journal)?;
    let value: i64 = conflicts
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get(0)
        .map_err(|_| HostError::Journal)?;
    if value != 0 {
        return Err(HostError::Journal);
    }
    Ok(())
}

pub(super) async fn latest(
    connection: &turso::Connection,
    identity: &RegistryImageIdentity,
) -> Result<Option<RegistryImageRecord>, HostError> {
    let mut rows = connection.query("SELECT id,attempt,state,runtime_id,receipt FROM image_import_registry WHERE engine_id=?1 AND docker_root_digest=?2 AND daemon_architecture=?3 AND source_sha=?4 AND release_manifest_sha256=?5 AND role=?6 AND registry_digest=?7 AND config_digest=?8 ORDER BY attempt DESC LIMIT 1", (identity.engine_id.as_str(), identity.docker_root_digest.as_str(), identity.daemon_architecture.as_str(), identity.source_sha.as_str(), identity.release_manifest_sha256.as_str(), role_text(identity.role), identity.registry_digest.as_str(), identity.config_digest.as_str())).await.map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(None);
    };
    let id = row.get(0).map_err(|_| HostError::Journal)?;
    let attempt = row.get(1).map_err(|_| HostError::Journal)?;
    let state = super::super::ImageImportState::parse(
        &row.get::<String>(2).map_err(|_| HostError::Journal)?,
    )?;
    let runtime_id: Option<String> = row.get(3).map_err(|_| HostError::Journal)?;
    let receipt: Option<String> = row.get(4).map_err(|_| HostError::Journal)?;
    let observation = match (state, runtime_id, receipt) {
        (super::super::ImageImportState::Completed, Some(runtime), Some(receipt)) => {
            let observation = RegistryImageObservation::from_receipt(&receipt, identity)?;
            if observation.runtime_id() != runtime {
                return Err(HostError::Journal);
            }
            Some(observation)
        }
        (super::super::ImageImportState::Completed, _, _) => return Err(HostError::Journal),
        (_, None, None) => None,
        _ => return Err(HostError::Journal),
    };
    Ok(Some(RegistryImageRecord {
        id,
        attempt,
        state,
        identity: identity.clone(),
        observation,
    }))
}

pub(super) fn parse_role(role: &str) -> Result<ImageKind, HostError> {
    match role {
        "runner" => Ok(ImageKind::Runner),
        "dind" => Ok(ImageKind::Dind),
        "resource-probe" => Ok(ImageKind::ResourceProbe),
        _ => Err(HostError::Journal),
    }
}
