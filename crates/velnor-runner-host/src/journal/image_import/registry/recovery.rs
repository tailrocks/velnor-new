// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{HostError, RegistryImageClaim, RegistryImageRecord, latest, role_text, verify_engine};
use crate::journal::{ImageImportState, Journal};

impl Journal {
    pub(crate) async fn resume_registry_image(
        &self,
        record: &RegistryImageRecord,
    ) -> Result<RegistryImageClaim, HostError> {
        crate::journal::connection::storage::read_transaction(self, async |connection| {
            verify_engine(connection, &record.identity).await?;
            let current = latest(connection, &record.identity)
                .await?
                .ok_or(HostError::Journal)?;
            if current.id != record.id || current.state != ImageImportState::Requested {
                return Err(HostError::Journal);
            }
            Ok(RegistryImageClaim {
                journal: self.clone(),
                id: current.id,
                identity: current.identity,
            })
        })
        .await
    }

    pub(crate) async fn reopen_registry_image(
        &self,
        record: &RegistryImageRecord,
    ) -> Result<RegistryImageRecord, HostError> {
        crate::journal::connection::storage::in_transaction(self, async |connection| {
            verify_engine(connection, &record.identity).await?;
            let current = latest(connection, &record.identity).await?.ok_or(HostError::Journal)?;
            if current.id != record.id || current.state != ImageImportState::Completed || super::has_unresolved(connection, &record.identity.engine_id).await? {
                return Err(HostError::Journal);
            }
            let attempt = current.attempt.checked_add(1).ok_or(HostError::Journal)?;
            let identity = &record.identity;
            connection.execute("INSERT INTO image_import_registry (engine_id,docker_root_digest,daemon_architecture,source_sha,release_manifest_sha256,role,registry_digest,config_digest,attempt,state,runtime_id,receipt) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,'prepared',NULL,NULL)", (identity.engine_id.as_str(), identity.docker_root_digest.as_str(), identity.daemon_architecture.as_str(), identity.source_sha.as_str(), identity.release_manifest_sha256.as_str(), role_text(identity.role), identity.registry_digest.as_str(), identity.config_digest.as_str(), attempt)).await.map_err(|_| HostError::Journal)?;
            latest(connection, identity).await?.ok_or(HostError::Journal)
        }).await
    }
}
