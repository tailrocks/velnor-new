use crate::error::HostError;
use crate::journal::connection::ConnectionJournal;
use crate::journal::{AssignmentIdentity, StorageProvisioningPlan};
use crate::worker::{WorkerIdentityNames, WorkerVolumeRole};

use super::ProvisionedRunner;

#[path = "provisioning/binding.rs"]
mod binding;

const BOOT_ID: &str = "11111111-2222-4333-8444-555555555555";

struct ProvisioningFixture<'a> {
    scope: &'a ConnectionJournal,
    assignment: AssignmentIdentity,
    nonce: String,
    reservation_id: i64,
    intent_id: i64,
    docker_root_digest: String,
    sample_generation_id: String,
}

pub(super) async fn provision(
    scope: &ConnectionJournal,
    assignment: AssignmentIdentity,
    nonce: char,
    reservation_id: i64,
    intent_id: i64,
    docker_root_digest: String,
    sample_generation_id: String,
) -> Result<ProvisionedRunner, HostError> {
    ProvisioningFixture {
        scope,
        assignment,
        nonce: nonce.to_string().repeat(32),
        reservation_id,
        intent_id,
        docker_root_digest,
        sample_generation_id,
    }
    .provision()
    .await
}

impl ProvisioningFixture<'_> {
    async fn provision(&self) -> Result<ProvisionedRunner, HostError> {
        let names = WorkerIdentityNames::from_nonce(&self.nonce)?;
        let profile = self.profile().await?;
        self.record_storage_objects(&profile, &names).await?;
        let (runner_id, dind_id, storage_plan) = self.record_bootstrap_containers(&names).await?;
        self.record_bootstrap_started(&runner_id, &dind_id).await?;
        let profile = self.profile().await?;
        let instance_binding_digest = self
            .record_storage_binding(&profile, &runner_id, &dind_id)
            .await?;
        Ok(ProvisionedRunner {
            assignment: self.assignment.clone(),
            intent_id: self.intent_id,
            reservation_id: self.reservation_id,
            runner_container_id: runner_id,
            dind_container_id: dind_id,
            docker_root_digest: self.docker_root_digest.clone(),
            sample_generation_id: self.sample_generation_id.clone(),
            instance_binding_digest,
            storage_plan,
            nonce: self.nonce.clone(),
        })
    }

    async fn profile(&self) -> Result<crate::journal::StorageProfile, HostError> {
        self.scope
            .current_storage_profile()
            .await?
            .ok_or(HostError::Journal)
    }

    async fn record_storage_objects(
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

    async fn record_storage_object(
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
            super::TEST_ENGINE_ID.to_owned(),
            self.docker_root_digest.clone(),
            role,
            name.clone(),
            name,
        )?;
        self.scope
            .record_storage_object_created(&claim, &proof)
            .await
    }

    async fn record_bootstrap_containers(
        &self,
        names: &WorkerIdentityNames,
    ) -> Result<(String, String, StorageProvisioningPlan), HostError> {
        use crate::launch::bootstrap::{BootstrapContainerRole, test_created_bootstrap_container};
        use crate::worker::WorkerContainerRole;

        let runner_id = "a".repeat(64);
        let dind_id = "b".repeat(64);
        let mut storage_plan = None;
        for (role, container_role, id) in [
            (
                BootstrapContainerRole::Runner,
                WorkerContainerRole::Runner,
                &runner_id,
            ),
            (
                BootstrapContainerRole::Dind,
                WorkerContainerRole::Dind,
                &dind_id,
            ),
        ] {
            let claim = self
                .scope
                .claim_bootstrap_container_create(self.reservation_id, role)
                .await?
                .ok_or(HostError::Journal)?;
            storage_plan.get_or_insert_with(|| claim.storage_plan().clone());
            let proof = test_created_bootstrap_container(
                self.reservation_id,
                self.intent_id,
                super::TEST_ENGINE_ID.to_owned(),
                self.docker_root_digest.clone(),
                &self.nonce,
                role,
                names.container_name(container_role),
                id.clone(),
            )?;
            self.scope
                .record_bootstrap_container_created(&claim, &proof)
                .await?;
        }
        Ok((runner_id, dind_id, storage_plan.ok_or(HostError::Journal)?))
    }

    async fn record_bootstrap_started(
        &self,
        runner_id: &str,
        dind_id: &str,
    ) -> Result<(), HostError> {
        use crate::launch::bootstrap::test_bootstrap_start_receipt;

        let start = self
            .scope
            .claim_bootstrap_start(self.reservation_id)
            .await?
            .ok_or(HostError::Journal)?;
        let proof = test_bootstrap_start_receipt(
            self.reservation_id,
            self.intent_id,
            super::TEST_ENGINE_ID.to_owned(),
            self.docker_root_digest.clone(),
            runner_id.to_owned(),
            dind_id.to_owned(),
            true,
            true,
        )?;
        self.scope.record_bootstrap_started(&start, &proof).await
    }
}
