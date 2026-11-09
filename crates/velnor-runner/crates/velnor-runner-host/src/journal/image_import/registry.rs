// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod recovery;
mod storage;
pub(in crate::journal) use storage::validate_rows;
use storage::{abort_unclaimed_archive_imports, latest};

use crate::error::HostError;
use velnor_runner_image_manifest::{ImageEntry, ImageKind};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RegistryImageIdentity {
    pub(crate) engine_id: String,
    pub(crate) docker_root_digest: String,
    pub(crate) daemon_architecture: String,
    pub(crate) source_sha: String,
    pub(crate) release_manifest_sha256: String,
    pub(crate) role: ImageKind,
    pub(crate) registry_digest: String,
    pub(crate) config_digest: String,
}

impl RegistryImageIdentity {
    pub(crate) fn new(
        engine_id: &str,
        docker_root_digest: &str,
        daemon_architecture: &str,
        source_sha: &str,
        release_manifest_sha256: &str,
        role: ImageKind,
        image: &ImageEntry,
    ) -> Result<Self, HostError> {
        if engine_id.is_empty()
            || engine_id.len() > 128
            || !engine_id
                .bytes()
                .all(|byte| byte.is_ascii_graphic() && !matches!(byte, b'\'' | b'"'))
            || !hex(docker_root_digest, 64)
            || !matches!(daemon_architecture, "x86_64" | "aarch64")
            || !hex(source_sha, 40)
            || !hex(release_manifest_sha256, 64)
            || image.repository != role.repository()
            || !digest(&image.registry_digest)
            || image.registry_digest != image.platform_manifest_digest
            || !digest(&image.config_digest)
            || image.registry_digest == image.config_digest
        {
            return Err(HostError::Identity);
        }
        Ok(Self {
            engine_id: engine_id.to_owned(),
            docker_root_digest: docker_root_digest.to_owned(),
            daemon_architecture: daemon_architecture.to_owned(),
            source_sha: source_sha.to_owned(),
            release_manifest_sha256: release_manifest_sha256.to_owned(),
            role,
            registry_digest: image.registry_digest.clone(),
            config_digest: image.config_digest.clone(),
        })
    }

    pub(crate) fn reference(&self) -> String {
        format!("{}@{}", self.role.repository(), self.registry_digest)
    }
}

fn digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|value| hex(value, 64))
}

fn hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "store", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum RegistryImageObservation {
    Classic {
        runtime_id: String,
        manifest_size: u64,
    },
    Containerd {
        runtime_id: String,
        manifest_size: u64,
    },
}

impl RegistryImageObservation {
    pub(crate) fn validate(&self, identity: &RegistryImageIdentity) -> Result<(), HostError> {
        identity.validate()?;
        let (runtime_id, size, expected) = match self {
            Self::Classic {
                runtime_id,
                manifest_size,
            } => (runtime_id, *manifest_size, identity.config_digest.as_str()),
            Self::Containerd {
                runtime_id,
                manifest_size,
            } => (
                runtime_id,
                *manifest_size,
                identity.registry_digest.as_str(),
            ),
        };
        if runtime_id != expected || size == 0 || size > 4 * 1024 * 1024 {
            return Err(HostError::Identity);
        }
        Ok(())
    }

    pub(crate) fn runtime_id(&self) -> &str {
        match self {
            Self::Classic { runtime_id, .. } | Self::Containerd { runtime_id, .. } => runtime_id,
        }
    }

    pub(crate) fn manifest_size(&self) -> u64 {
        match self {
            Self::Classic { manifest_size, .. } | Self::Containerd { manifest_size, .. } => {
                *manifest_size
            }
        }
    }

    fn receipt(&self) -> Result<String, HostError> {
        serde_json::to_string(self).map_err(|_| HostError::Journal)
    }

    fn from_receipt(value: &str, identity: &RegistryImageIdentity) -> Result<Self, HostError> {
        let observation: Self = serde_json::from_str(value).map_err(|_| HostError::Journal)?;
        if observation.receipt()? != value {
            return Err(HostError::Journal);
        }
        observation
            .validate(identity)
            .map_err(|_| HostError::Journal)?;
        Ok(observation)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RegistryImageInspection {
    identity: RegistryImageIdentity,
    observation: RegistryImageObservation,
}

impl RegistryImageInspection {
    pub(crate) fn verified(
        identity: &RegistryImageIdentity,
        observed_engine: &str,
        observed_root: &str,
        observed_arch: &str,
        observation: RegistryImageObservation,
    ) -> Result<Self, HostError> {
        if identity.engine_id != observed_engine
            || identity.docker_root_digest != observed_root
            || identity.daemon_architecture != observed_arch
        {
            return Err(HostError::Identity);
        }
        observation.validate(identity)?;
        Ok(Self {
            identity: identity.clone(),
            observation,
        })
    }
    pub(crate) fn observation(&self) -> &RegistryImageObservation {
        &self.observation
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RegistryImageRecord {
    id: i64,
    pub(crate) attempt: i64,
    pub(crate) state: super::ImageImportState,
    pub(crate) identity: RegistryImageIdentity,
    pub(crate) observation: Option<RegistryImageObservation>,
}

#[derive(Debug)]
pub(crate) struct RegistryImageClaim {
    journal: crate::journal::Journal,
    id: i64,
    identity: RegistryImageIdentity,
}

impl RegistryImageIdentity {
    fn validate(&self) -> Result<(), HostError> {
        if self.engine_id.is_empty()
            || self.engine_id.len() > 128
            || !self
                .engine_id
                .bytes()
                .all(|byte| byte.is_ascii_graphic() && !matches!(byte, b'\'' | b'"'))
            || !hex(&self.docker_root_digest, 64)
            || !matches!(self.daemon_architecture.as_str(), "x86_64" | "aarch64")
            || !hex(&self.source_sha, 40)
            || !hex(&self.release_manifest_sha256, 64)
            || !digest(&self.registry_digest)
            || !digest(&self.config_digest)
            || self.registry_digest == self.config_digest
        {
            return Err(HostError::Identity);
        }
        Ok(())
    }
}

fn role_text(role: ImageKind) -> &'static str {
    match role {
        ImageKind::Runner => "runner",
        ImageKind::Dind => "dind",
        ImageKind::ResourceProbe => "resource-probe",
    }
}

impl crate::journal::Journal {
    pub(crate) async fn ensure_image_imports_resolved(
        &self,
        engine_id: &str,
    ) -> Result<(), HostError> {
        crate::journal::connection::storage::in_transaction(self, async |connection| {
            if crate::journal_schema::engine_id(connection).await? != engine_id
                || has_unresolved(connection, engine_id).await?
            {
                return Err(HostError::Journal);
            }
            Ok(())
        })
        .await
    }

    pub(crate) async fn prepare_registry_image(
        &self,
        identity: RegistryImageIdentity,
    ) -> Result<RegistryImageRecord, HostError> {
        identity.validate()?;
        crate::journal::connection::storage::in_transaction(self, async |connection| {
            verify_engine(connection, &identity).await?;
            abort_unclaimed_archive_imports(connection, &identity.engine_id).await?;
            let existing = latest(connection, &identity).await?;
            if let Some(record) = existing.as_ref().filter(|record| record.state != super::ImageImportState::Aborted) {
                if has_other_unresolved(connection, identity.engine_id.as_str(), record.id).await? { return Err(HostError::Journal); }
                return Ok(record.clone());
            }
            if has_unresolved(connection, identity.engine_id.as_str()).await? { return Err(HostError::Journal); }
            let attempt = existing.map_or(Ok(0), |record| record.attempt.checked_add(1).ok_or(HostError::Journal))?;
            connection.execute("INSERT INTO image_import_registry (engine_id,docker_root_digest,daemon_architecture,source_sha,release_manifest_sha256,role,registry_digest,config_digest,attempt,state,runtime_id,receipt) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,'prepared',NULL,NULL)", (identity.engine_id.as_str(), identity.docker_root_digest.as_str(), identity.daemon_architecture.as_str(), identity.source_sha.as_str(), identity.release_manifest_sha256.as_str(), role_text(identity.role), identity.registry_digest.as_str(), identity.config_digest.as_str(), attempt)).await.map_err(|_| HostError::Journal)?;
            latest(connection, &identity).await?.ok_or(HostError::Journal)
        }).await
    }

    pub(crate) async fn claim_registry_image(
        &self,
        record: &RegistryImageRecord,
    ) -> Result<Option<RegistryImageClaim>, HostError> {
        record.identity.validate()?;
        crate::journal::connection::storage::in_transaction(self, async |connection| {
            verify_engine(connection, &record.identity).await?;
            let current = latest(connection, &record.identity).await?.ok_or(HostError::Journal)?;
            if current.id != record.id || current.state != super::ImageImportState::Prepared { return Ok(None); }
            if has_other_unresolved(connection, &record.identity.engine_id, current.id).await? { return Err(HostError::Journal); }
            let count = connection.execute("UPDATE image_import_registry SET state='requested' WHERE id=?1 AND state='prepared'", [record.id]).await.map_err(|_| HostError::Journal)?;
            crate::journal::sql::one_row(count)?;
            Ok(Some(RegistryImageClaim { journal: self.clone(), id: record.id, identity: record.identity.clone() }))
        }).await
    }

    pub(crate) async fn mark_registry_image_uncertain(
        &self,
        claim: RegistryImageClaim,
    ) -> Result<(), HostError> {
        if self != &claim.journal {
            return Err(HostError::Identity);
        }
        crate::journal::connection::storage::in_transaction(self, async |connection| {
            verify_engine(connection, &claim.identity).await?;
            let current = latest(connection, &claim.identity).await?.ok_or(HostError::Journal)?;
            if current.id != claim.id || current.state != super::ImageImportState::Requested { return Err(HostError::Journal); }
            let count = connection.execute("UPDATE image_import_registry SET state='uncertain' WHERE id=?1 AND state='requested'", [claim.id]).await.map_err(|_| HostError::Journal)?;
            crate::journal::sql::one_row(count)
        }).await
    }

    pub(crate) async fn complete_registry_image(
        &self,
        claim: RegistryImageClaim,
        inspection: &RegistryImageInspection,
    ) -> Result<(), HostError> {
        if self != &claim.journal || inspection.identity != claim.identity {
            return Err(HostError::Identity);
        }
        let observation = &inspection.observation;
        observation.validate(&claim.identity)?;
        let receipt = observation.receipt()?;
        crate::journal::connection::storage::in_transaction(self, async |connection| {
            verify_engine(connection, &claim.identity).await?;
            let current = latest(connection, &claim.identity).await?.ok_or(HostError::Journal)?;
            if current.id != claim.id || current.state != super::ImageImportState::Requested { return Err(HostError::Journal); }
            let count = connection.execute("UPDATE image_import_registry SET state='completed', runtime_id=?1, receipt=?2 WHERE id=?3 AND state='requested'", (observation.runtime_id(), receipt.as_str(), claim.id)).await.map_err(|_| HostError::Journal)?;
            crate::journal::sql::one_row(count)
        }).await
    }
    pub(crate) async fn complete_preloaded_registry_image(
        &self,
        record: &RegistryImageRecord,
        inspection: &RegistryImageInspection,
    ) -> Result<(), HostError> {
        if record.identity != inspection.identity {
            return Err(HostError::Identity);
        }
        inspection.observation.validate(&record.identity)?;
        let receipt = inspection.observation.receipt()?;
        crate::journal::connection::storage::in_transaction(self, async |connection| {
            verify_engine(connection, &record.identity).await?;
            let current = latest(connection, &record.identity).await?.ok_or(HostError::Journal)?;
            if current.id != record.id || current.state != super::ImageImportState::Prepared || has_other_unresolved(connection, &record.identity.engine_id, current.id).await? { return Err(HostError::Journal); }
            let count = connection.execute("UPDATE image_import_registry SET state='completed', runtime_id=?1, receipt=?2 WHERE id=?3 AND state='prepared'", (inspection.observation.runtime_id(), receipt.as_str(), current.id)).await.map_err(|_| HostError::Journal)?;
            crate::journal::sql::one_row(count)
        }).await
    }

    pub(crate) async fn verify_completed_registry_image(
        &self,
        record: &RegistryImageRecord,
        inspection: &RegistryImageInspection,
    ) -> Result<(), HostError> {
        if record.identity != inspection.identity {
            return Err(HostError::Identity);
        }
        inspection.observation.validate(&record.identity)?;
        crate::journal::connection::storage::read_transaction(self, async |connection| {
            verify_engine(connection, &record.identity).await?;
            let current = latest(connection, &record.identity)
                .await?
                .ok_or(HostError::Journal)?;
            if current.id != record.id
                || current.state != super::ImageImportState::Completed
                || current.observation.as_ref() != Some(&inspection.observation)
            {
                return Err(HostError::Identity);
            }
            Ok(())
        })
        .await
    }
}

async fn verify_engine(
    connection: &turso::Connection,
    identity: &RegistryImageIdentity,
) -> Result<(), HostError> {
    if crate::journal_schema::engine_id(connection).await? == identity.engine_id {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}

pub(super) async fn has_unresolved(
    connection: &turso::Connection,
    engine_id: &str,
) -> Result<bool, HostError> {
    let mut rows = connection.query("SELECT EXISTS (SELECT 1 FROM image_imports WHERE engine_id=?1 AND state IN ('prepared','requested','uncertain')) OR EXISTS (SELECT 1 FROM image_import_registry WHERE engine_id=?1 AND state IN ('prepared','requested','uncertain'))", [engine_id]).await.map_err(|_| HostError::Journal)?;
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
    engine_id: &str,
    excluded_id: i64,
) -> Result<bool, HostError> {
    let mut rows = connection.query("SELECT EXISTS (SELECT 1 FROM image_imports WHERE engine_id=?1 AND state IN ('prepared','requested','uncertain')) OR EXISTS (SELECT 1 FROM image_import_registry WHERE engine_id=?1 AND id!=?2 AND state IN ('prepared','requested','uncertain'))", (engine_id, excluded_id)).await.map_err(|_| HostError::Journal)?;
    let value: i64 = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get(0)
        .map_err(|_| HostError::Journal)?;
    Ok(value != 0)
}
