use super::ProvisioningFixture;
use crate::error::HostError;
use crate::worker::{WorkerIdentityNames, WorkerVolumeRole};

impl ProvisioningFixture<'_> {
    pub(super) async fn profile(&self) -> Result<crate::journal::StorageProfile, HostError> {
        self.scope
            .current_storage_profile()
            .await?
            .ok_or(HostError::Journal)
    }

    pub(super) async fn record_storage_objects(
        &self,
        profile: &crate::journal::StorageProfile,
        names: &WorkerIdentityNames,
    ) -> Result<(), HostError> {
        use crate::journal::StorageRole;
        for (role, volume_role) in [
            (StorageRole::SharedRun, WorkerVolumeRole::Socket),
            (StorageRole::RunnerHome, WorkerVolumeRole::Home),
            (StorageRole::RunnerWork, WorkerVolumeRole::Work),
            (StorageRole::DindData, WorkerVolumeRole::DindData),
            (
                StorageRole::EngineMetadata,
                WorkerVolumeRole::RunnerMetadata,
            ),
        ] {
            self.record_storage_object(profile, names, role, volume_role)
                .await?;
        }
        Ok(())
    }

    pub(super) async fn record_storage_object(
        &self,
        profile: &crate::journal::StorageProfile,
        names: &WorkerIdentityNames,
        role: crate::journal::StorageRole,
        volume_role: WorkerVolumeRole,
    ) -> Result<(), HostError> {
        use crate::journal::{StorageFilesystemKind, StorageInodePolicy, StorageRole};
        use crate::launch::bootstrap::test_created_storage_object;

        let claim = self
            .scope
            .claim_storage_object_create(self.reservation_id, role)
            .await?
            .ok_or(HostError::Journal)?;
        let role_limit = profile.role_limit(role)?;
        let pool = profile.pool_limit(role_limit.pool_id())?;
        let name = names.volume_name(volume_role);
        let expected_kind = if role == StorageRole::EngineMetadata {
            StorageFilesystemKind::Btrfs
        } else {
            StorageFilesystemKind::Tmpfs
        };
        let expected_inode_policy = if role == StorageRole::EngineMetadata {
            StorageInodePolicy::DynamicBtrfs
        } else {
            StorageInodePolicy::Finite
        };
        let publication_matches = if role == StorageRole::EngineMetadata {
            match profile.controller_publication() {
                Some(policy) => {
                    let digest = policy.digest();
                    claim.publication_policy_digest() == Some(digest.as_str())
                }
                None => false,
            }
        } else {
            claim.publication_policy_digest().is_none()
        };
        if claim.docker_name() != name
            || claim.profile_generation_id() != profile.generation_id()
            || claim.profile_digest() != profile.profile_digest()
            || claim.sample_generation_id() != self.sample_generation_id
            || claim.pool_id() != role_limit.pool_id()
            || claim.medium() != role_limit.medium()
            || claim.filesystem_kind() != expected_kind
            || claim.inode_policy() != expected_inode_policy
            || claim.charge_bytes() != role_limit.max_bytes()
            || claim.charge_inodes() != role_limit.max_inodes()
            || claim.retained_memory_bytes() != role_limit.retained_memory_bytes()
            || pool.inode_policy() != claim.inode_policy()
            || !publication_matches
        {
            return Err(HostError::Journal);
        }
        let proof = test_created_storage_object(
            self.reservation_id,
            self.intent_id,
            self.engine_id.clone(),
            self.docker_root_digest.clone(),
            role,
            name.clone(),
            name,
        )?;
        self.scope
            .record_storage_object_created(&claim, &proof)
            .await
    }
}
