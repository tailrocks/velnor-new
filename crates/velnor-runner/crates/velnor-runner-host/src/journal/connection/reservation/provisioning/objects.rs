use crate::error::HostError;
use crate::journal::connection::storage::in_transaction;
use crate::journal::connection::{
    ConnectionJournal, StorageFilesystemKind, StorageInodePolicy, StorageMedium, StorageRole,
};
use crate::launch::bootstrap::VerifiedCreatedStorageObject;

use super::{hold_context, hold_is_claimable, mark_hold_uncertain};
use role_plan::load_object_role_plan;

mod role_plan;

/// One durable create attempt for a named tmpfs volume.
pub(crate) struct StorageObjectCreateClaim {
    connection_id: String,
    reservation_id: i64,
    operation_id: i64,
    engine_id: String,
    docker_root_digest: String,
    role: StorageRole,
    docker_name: String,
    profile_generation_id: String,
    profile_digest: String,
    sample_generation_id: String,
    pool_id: String,
    medium: StorageMedium,
    filesystem_kind: StorageFilesystemKind,
    inode_policy: StorageInodePolicy,
    charge_bytes: u64,
    charge_inodes: u64,
    retained_memory_bytes: u64,
    publication_policy_digest: Option<String>,
}

impl StorageObjectCreateClaim {
    pub(crate) const fn reservation_id(&self) -> i64 {
        self.reservation_id
    }

    pub(crate) const fn operation_id(&self) -> i64 {
        self.operation_id
    }

    pub(crate) const fn role(&self) -> StorageRole {
        self.role
    }

    pub(crate) fn docker_name(&self) -> &str {
        &self.docker_name
    }

    pub(crate) fn engine_id(&self) -> &str {
        &self.engine_id
    }

    pub(crate) fn docker_root_digest(&self) -> &str {
        &self.docker_root_digest
    }

    #[cfg(test)]
    pub(crate) fn profile_generation_id(&self) -> &str {
        &self.profile_generation_id
    }

    #[cfg(test)]
    pub(crate) fn profile_digest(&self) -> &str {
        &self.profile_digest
    }

    pub(crate) fn sample_generation_id(&self) -> &str {
        &self.sample_generation_id
    }

    #[cfg(test)]
    pub(crate) fn pool_id(&self) -> &str {
        &self.pool_id
    }

    pub(crate) const fn medium(&self) -> StorageMedium {
        self.medium
    }

    pub(crate) const fn filesystem_kind(&self) -> StorageFilesystemKind {
        self.filesystem_kind
    }

    pub(crate) const fn inode_policy(&self) -> StorageInodePolicy {
        self.inode_policy
    }

    pub(crate) const fn charge_bytes(&self) -> u64 {
        self.charge_bytes
    }

    pub(crate) const fn charge_inodes(&self) -> u64 {
        self.charge_inodes
    }

    pub(crate) const fn retained_memory_bytes(&self) -> u64 {
        self.retained_memory_bytes
    }

    #[cfg(test)]
    pub(crate) fn publication_policy_digest(&self) -> Option<&str> {
        self.publication_policy_digest.as_deref()
    }
}

impl ConnectionJournal {
    /// Record permission to create one volume before you send the Docker request.
    pub(crate) async fn claim_storage_object_create(
        &self,
        reservation_id: i64,
        role: StorageRole,
    ) -> Result<Option<StorageObjectCreateClaim>, HostError> {
        if reservation_id <= 0 || !object_role(role) {
            return Err(HostError::Identity);
        }
        let connection_id = self.connection_id().as_str().to_owned();
        in_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            let Some(context) =
                hold_context(connection, &connection_id, reservation_id).await?
            else {
                return Ok(None);
            };
            if !hold_is_claimable(&context) {
                return Ok(None);
            }
            let plan = load_object_role_plan(connection, reservation_id, role, &context).await?;
            let mut rows = connection
                .query(
                    "SELECT docker_name FROM capacity_reservation_provisioning_objects WHERE reservation_id = ?1 AND role = ?2 AND create_state = 'unclaimed' AND docker_id IS NULL",
                    (reservation_id, role.as_str()),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
                return Ok(None);
            };
            let docker_name: String = row.get(0).map_err(|_| HostError::Journal)?;
            let changed = connection
                .execute(
                    "UPDATE capacity_reservation_provisioning_objects SET create_state = 'requested' WHERE reservation_id = ?1 AND role = ?2 AND create_state = 'unclaimed' AND docker_id IS NULL",
                    (reservation_id, role.as_str()),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if changed != 1 {
                return Ok(None);
            }
            Ok(Some(StorageObjectCreateClaim {
                connection_id: connection_id.clone(),
                reservation_id,
                operation_id: context.operation_id,
                engine_id: context.engine_id,
                docker_root_digest: context.docker_root_digest,
                role,
                docker_name,
                profile_generation_id: context.profile_generation_id,
                profile_digest: context.profile_digest,
                sample_generation_id: context.sample_generation_id,
                pool_id: plan.pool_id,
                medium: plan.medium,
                filesystem_kind: plan.filesystem_kind,
                inode_policy: plan.inode_policy,
                charge_bytes: plan.charge_bytes,
                charge_inodes: plan.charge_inodes,
                retained_memory_bytes: plan.retained_memory_bytes,
                publication_policy_digest: plan.publication_policy_digest,
            }))
        })
        .await
    }

    /// Record exact create and inspect evidence before another object is created.
    pub(crate) async fn record_storage_object_created(
        &self,
        claim: &StorageObjectCreateClaim,
        proof: &VerifiedCreatedStorageObject,
    ) -> Result<(), HostError> {
        validate_claim(self, claim)?;
        if proof.reservation_id() != claim.reservation_id
            || proof.operation_id() != claim.operation_id
            || proof.engine_id() != claim.engine_id
            || proof.docker_root_digest() != claim.docker_root_digest
            || proof.role() != claim.role
            || proof.docker_name() != claim.docker_name
            || proof.docker_id() != claim.docker_name
        {
            return Err(HostError::Identity);
        }
        in_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            let Some(context) = hold_context(
                connection,
                claim.connection_id.as_str(),
                claim.reservation_id,
            )
            .await?
            else {
                return Err(HostError::Journal);
            };
            if context.operation_id != claim.operation_id
                || context.engine_id != claim.engine_id
                || context.docker_root_digest != claim.docker_root_digest
                || context.reservation_state != "reserved"
                || context.intent_state != "pending"
            {
                return Err(HostError::Journal);
            }
            let changed = connection
                .execute(
                    "UPDATE capacity_reservation_provisioning_objects SET create_state = 'created', docker_id = ?1 WHERE reservation_id = ?2 AND role = ?3 AND docker_name = ?1 AND create_state = 'requested' AND docker_id IS NULL",
                    (claim.docker_name.as_str(), claim.reservation_id, claim.role.as_str()),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if changed == 1 {
                Ok(())
            } else {
                Err(HostError::Journal)
            }
        })
        .await
    }

    /// If a volume creation has no exact recorded result, keep the full capacity reservation.
    pub(crate) async fn mark_storage_object_create_uncertain(
        &self,
        claim: &StorageObjectCreateClaim,
    ) -> Result<(), HostError> {
        validate_claim(self, claim)?;
        in_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            let changed = connection
                .execute(
                    "UPDATE capacity_reservation_provisioning_objects SET create_state = 'uncertain' WHERE reservation_id = ?1 AND role = ?2 AND docker_name = ?3 AND create_state = 'requested' AND docker_id IS NULL",
                    (claim.reservation_id, claim.role.as_str(), claim.docker_name.as_str()),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if changed != 1 {
                return Err(HostError::Journal);
            }
            mark_hold_uncertain(
                connection,
                &claim.connection_id,
                claim.reservation_id,
                claim.operation_id,
            )
            .await
        })
        .await
    }
}

fn validate_claim(
    journal: &ConnectionJournal,
    claim: &StorageObjectCreateClaim,
) -> Result<(), HostError> {
    if journal.connection_id().as_str() != claim.connection_id
        || claim.reservation_id <= 0
        || claim.operation_id <= 0
        || !object_role(claim.role)
        || claim.docker_name.is_empty()
        || claim.docker_name.len() > 128
        || claim
            .docker_name
            .bytes()
            .any(|byte| byte.is_ascii_control())
    {
        Err(HostError::Identity)
    } else {
        Ok(())
    }
}

const fn object_role(role: StorageRole) -> bool {
    matches!(
        role,
        StorageRole::SharedRun
            | StorageRole::RunnerHome
            | StorageRole::RunnerWork
            | StorageRole::DindData
            | StorageRole::EngineMetadata
    )
}
