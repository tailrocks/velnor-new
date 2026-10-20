use crate::error::HostError;
use crate::journal::StorageProfile;

use super::ProvisioningFixture;

impl ProvisioningFixture<'_> {
    pub(super) async fn record_storage_binding(
        &self,
        profile: &StorageProfile,
        runner_id: &str,
        dind_id: &str,
    ) -> Result<String, HostError> {
        use crate::launch::bootstrap::test_verified_storage_resource_binding;

        let (roles, role_digests) = Self::observed_roles(profile)?;
        let attachments = Self::observed_attachments(profile, runner_id, dind_id, &role_digests)?;
        let cgroups = Self::observed_cgroups(runner_id, dind_id)?;
        let instance_binding_digest = "f".repeat(64);
        let binding = test_verified_storage_resource_binding(
            self.reservation_id,
            self.intent_id,
            self.engine_id.clone(),
            self.docker_root_digest.clone(),
            self.sample_generation_id.clone(),
            profile.generation_id().to_owned(),
            profile.profile_digest().to_owned(),
            runner_id.to_owned(),
            dind_id.to_owned(),
            self.scope.connection_id().as_str().to_owned(),
            instance_binding_digest.clone(),
            roles,
            attachments,
            cgroups,
            super::BOOT_ID.to_owned(),
        )?;
        if self
            .scope
            .bind_storage_resources(self.reservation_id + 1, &binding)
            .await
            .is_ok()
        {
            return Err(HostError::Journal);
        }
        self.scope
            .bind_storage_resources(self.reservation_id, &binding)
            .await?;
        if self
            .scope
            .bind_storage_resources(self.reservation_id, &binding)
            .await
            .is_ok()
        {
            return Err(HostError::Journal);
        }
        Ok(instance_binding_digest)
    }

    fn observed_roles(
        profile: &StorageProfile,
    ) -> Result<
        (
            Vec<crate::launch::bootstrap::ObservedStorageRole>,
            std::collections::BTreeMap<crate::journal::StorageRole, String>,
        ),
        HostError,
    > {
        use crate::journal::{
            StorageFilesystemKind, StorageInodePolicy, StorageMedium, StorageRole,
        };
        use crate::launch::bootstrap::test_observed_storage_role;

        let mut roles = Vec::with_capacity(StorageRole::PHYSICAL.len());
        let mut role_digests = std::collections::BTreeMap::new();
        for role in StorageRole::PHYSICAL {
            let limit = profile.role_limit(role)?;
            let pool = profile.pool_limit(limit.pool_id())?;
            let digest = super::super::role_filesystem_digest(role);
            let (total, free, available) = match limit.medium() {
                StorageMedium::Disk => (64_u64 << 30, 60_u64 << 30, 60_u64 << 30),
                StorageMedium::Tmpfs => (limit.max_bytes(), limit.max_bytes(), limit.max_bytes()),
            };
            let inode_counts = match pool.inode_policy() {
                StorageInodePolicy::DynamicBtrfs => (Some(0), Some(0), Some(0)),
                StorageInodePolicy::Finite => (
                    Some(limit.max_inodes()),
                    Some(limit.max_inodes()),
                    Some(limit.max_inodes()),
                ),
                StorageInodePolicy::Unknown => return Err(HostError::Journal),
            };
            let filesystem_kind = if limit.medium() == StorageMedium::Disk {
                StorageFilesystemKind::Btrfs
            } else {
                StorageFilesystemKind::Tmpfs
            };
            roles.push(test_observed_storage_role(
                role,
                limit.pool_id().to_owned(),
                digest.clone(),
                limit.medium(),
                filesystem_kind,
                (total, free, available),
                inode_counts,
                pool.inode_policy(),
                (
                    limit.max_bytes(),
                    limit.max_inodes(),
                    limit.retained_memory_bytes(),
                ),
                (0, 0),
            )?);
            role_digests.insert(role, digest);
        }
        Ok((roles, role_digests))
    }

    fn observed_attachments(
        profile: &StorageProfile,
        runner_id: &str,
        dind_id: &str,
        role_digests: &std::collections::BTreeMap<crate::journal::StorageRole, String>,
    ) -> Result<Vec<crate::launch::bootstrap::ObservedStorageAttachment>, HostError> {
        use crate::journal::{StorageMedium, StorageObserver, StorageRole};
        use crate::launch::bootstrap::test_observed_storage_attachment;

        let mut attachments = Vec::new();
        for role in StorageRole::PHYSICAL {
            for (index, contract) in role.attachments().iter().enumerate() {
                let container_id = match contract.observer {
                    StorageObserver::Runner => Some(runner_id.to_owned()),
                    StorageObserver::Dind => Some(dind_id.to_owned()),
                    StorageObserver::Probe => None,
                };
                let disk = profile.role_limit(role)?.medium() == StorageMedium::Disk;
                let (device, statx, magic, filesystem_id) = if disk {
                    let device = if role == StorageRole::EngineMetadata
                        && contract.observer == StorageObserver::Probe
                    {
                        (0, 41)
                    } else {
                        (0, 38)
                    };
                    (device, (0, 41), 0x9123_683e, 11_u64)
                } else {
                    let role_index = StorageRole::PHYSICAL
                        .iter()
                        .position(|item| *item == role)
                        .ok_or(HostError::Journal)?;
                    let fs = i64::try_from(role_index)
                        .map_err(|_| HostError::Journal)?
                        .checked_add(30)
                        .ok_or(HostError::Journal)?;
                    let device_minor = 45_u32
                        .checked_add(u32::try_from(role_index).map_err(|_| HostError::Journal)?)
                        .ok_or(HostError::Journal)?;
                    (
                        (0, device_minor),
                        (0, device_minor),
                        0x0102_1994,
                        u64::try_from(fs).map_err(|_| HostError::Journal)?,
                    )
                };
                attachments.push(test_observed_storage_attachment(
                    role,
                    contract.observer,
                    container_id,
                    contract.target.to_owned(),
                    contract.source_target.map(str::to_owned),
                    contract
                        .volume_subpath
                        .map(|subpath| format!("/volume/{subpath}"))
                        .unwrap_or_else(|| "/".to_owned()),
                    contract.read_only,
                    100 + (role as u64 * 2) + index as u64,
                    500 + (role as u64 * 2)
                        + if role == StorageRole::SharedRun {
                            0
                        } else {
                            index as u64
                        },
                    device,
                    statx,
                    magic,
                    filesystem_id,
                    role_digests.get(&role).ok_or(HostError::Journal)?.clone(),
                )?);
            }
        }
        Ok(attachments)
    }

    fn observed_cgroups(
        runner_id: &str,
        dind_id: &str,
    ) -> Result<Vec<crate::launch::bootstrap::ObservedContainerCgroup>, HostError> {
        use crate::launch::bootstrap::{BootstrapContainerRole, test_observed_container_cgroup};

        Ok(vec![
            test_observed_container_cgroup(
                BootstrapContainerRole::Runner,
                runner_id.to_owned(),
                format!("/test/runner/{runner_id}"),
                (Some(6_u64 << 30), 1_u64 << 20),
                (Some(100_000), 100_000),
                Some(2_048),
            )?,
            test_observed_container_cgroup(
                BootstrapContainerRole::Dind,
                dind_id.to_owned(),
                format!("/test/dind/{dind_id}"),
                (Some(2_u64 << 30), 1_u64 << 20),
                (Some(100_000), 100_000),
                Some(2_048),
            )?,
        ])
    }
}
