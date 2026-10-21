use crate::error::HostError;
use crate::journal::StorageProfile;

use super::ProvisioningFixture;

#[derive(Clone, Copy)]
struct ObservedAttachmentInput {
    role: crate::journal::StorageRole,
    index: usize,
    observer: crate::journal::StorageObserver,
    target: &'static str,
    source_target: Option<&'static str>,
    volume_subpath: Option<&'static str>,
    read_only: bool,
}

struct ObservedDeviceIdentity {
    device: (u32, u32),
    statx: (u32, u32),
    magic: u64,
    filesystem_id: u64,
}

impl ProvisioningFixture<'_> {
    pub(super) async fn record_storage_binding(
        &self,
        profile: &StorageProfile,
        runner_id: &str,
        dind_id: &str,
    ) -> Result<String, HostError> {
        use crate::launch::bootstrap::{
            VerifiedStorageResourceBindingFixture, test_verified_storage_resource_binding,
        };

        let (roles, role_digests) = self.observed_roles(profile)?;
        let attachments = Self::observed_attachments(profile, runner_id, dind_id, &role_digests)?;
        let cgroups = Self::observed_cgroups(runner_id, dind_id)?;
        let instance_binding_digest = "f".repeat(64);
        let binding = test_verified_storage_resource_binding(VerifiedStorageResourceBindingFixture {
            reservation_id: self.reservation_id,
            operation_id: self.intent_id,
            engine_id: self.engine_id.clone(),
            docker_root_digest: self.docker_root_digest.clone(),
            sample_generation_id: self.sample_generation_id.clone(),
            profile_generation_id: profile.generation_id().to_owned(),
            profile_digest: profile.profile_digest().to_owned(),
            runner_container_id: runner_id.to_owned(),
            dind_container_id: dind_id.to_owned(),
            connection_id: self.scope.connection_id().as_str().to_owned(),
            instance_binding_digest: instance_binding_digest.clone(),
            roles,
            attachments,
            cgroups,
            guest_boot_id: super::BOOT_ID.to_owned(),
        })?;
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
        &self,
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
            let digest = match role {
                StorageRole::RunnerRoot | StorageRole::DindRoot | StorageRole::EngineMetadata => {
                    self.filesystem_instance_digest.clone()
                }
                _ => super::super::role_filesystem_digest(role),
            };
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
        use crate::journal::StorageRole;

        let mut attachments = Vec::new();
        for role in StorageRole::PHYSICAL {
            for (index, contract) in role.attachments().iter().enumerate() {
                attachments.push(Self::observed_attachment(
                    profile,
                    runner_id,
                    dind_id,
                    role_digests,
                    ObservedAttachmentInput {
                        role,
                        index,
                        observer: contract.observer,
                        target: contract.target,
                        source_target: contract.source_target,
                        volume_subpath: contract.volume_subpath,
                        read_only: contract.read_only,
                    },
                )?);
            }
        }
        Ok(attachments)
    }

    fn observed_attachment(
        profile: &StorageProfile,
        runner_id: &str,
        dind_id: &str,
        role_digests: &std::collections::BTreeMap<crate::journal::StorageRole, String>,
        input: ObservedAttachmentInput,
    ) -> Result<crate::launch::bootstrap::ObservedStorageAttachment, HostError> {
        use crate::journal::{StorageObserver, StorageRole};
        use crate::launch::bootstrap::test_observed_storage_attachment;

        let ObservedAttachmentInput {
            role,
            index,
            observer,
            target,
            source_target,
            volume_subpath,
            read_only,
        } = input;
        let container_id = match observer {
            StorageObserver::Runner => Some(runner_id.to_owned()),
            StorageObserver::Dind => Some(dind_id.to_owned()),
            StorageObserver::Probe => None,
        };
        let ObservedDeviceIdentity {
            device,
            statx,
            magic,
            filesystem_id,
        } = Self::attachment_devices(profile, role, observer)?;
        let inode = Self::attachment_inode(role, index, target)?;
        let filesystem_digest = if role == StorageRole::EngineMetadata
            && observer != StorageObserver::Probe
        {
            "b".repeat(64)
        } else {
            role_digests.get(&role).ok_or(HostError::Journal)?.clone()
        };
        test_observed_storage_attachment(
            role,
            observer,
            container_id,
            target.to_owned(),
            source_target.map(str::to_owned),
            volume_subpath
                .map_or_else(|| "/".to_owned(), |subpath| format!("/volume/{subpath}")),
            read_only,
            100 + (role as u64 * 2) + index as u64,
            inode,
            device,
            statx,
            magic,
            filesystem_id,
            filesystem_digest,
        )
    }

    fn attachment_devices(
        profile: &StorageProfile,
        role: crate::journal::StorageRole,
        observer: crate::journal::StorageObserver,
    ) -> Result<ObservedDeviceIdentity, HostError> {
        use crate::journal::{StorageMedium, StorageObserver, StorageRole};

        if profile.role_limit(role)?.medium() == StorageMedium::Disk {
            return Ok(if role == StorageRole::EngineMetadata && observer != StorageObserver::Probe {
                ObservedDeviceIdentity {
                    device: (0, 48),
                    statx: (0, 48),
                    magic: 0x9123_683e,
                    filesystem_id: 12,
                }
            } else {
                let device = if role == StorageRole::EngineMetadata {
                    (0, 41)
                } else {
                    (0, 38)
                };
                ObservedDeviceIdentity {
                    device,
                    statx: (0, 41),
                    magic: 0x9123_683e,
                    filesystem_id: 11,
                }
            });
        }
        let role_index = StorageRole::PHYSICAL
            .iter()
            .position(|item| *item == role)
            .ok_or(HostError::Journal)?;
        let filesystem_id = i64::try_from(role_index)
            .map_err(|_| HostError::Journal)?
            .checked_add(30)
            .ok_or(HostError::Journal)?;
        let device_minor = 45_u32
            .checked_add(u32::try_from(role_index).map_err(|_| HostError::Journal)?)
            .ok_or(HostError::Journal)?;
        Ok(ObservedDeviceIdentity {
            device: (0, device_minor),
            statx: (0, device_minor),
            magic: 0x0102_1994,
            filesystem_id: u64::try_from(filesystem_id).map_err(|_| HostError::Journal)?,
        })
    }

    fn attachment_inode(
        role: crate::journal::StorageRole,
        index: usize,
        target: &str,
    ) -> Result<u64, HostError> {
        use crate::journal::StorageRole;

        if role == StorageRole::EngineMetadata {
            return match target {
                "/velnor/docker-root" => Ok(41),
                "/.velnor-runtime-identity" => Ok(64),
                "/opt/velnor/job-facts" => Ok(65),
                "/opt/velnor/source-proof/control" => Ok(67),
                _ => Err(HostError::Journal),
            };
        }
        Ok(500 + (role as u64 * 2)
            + match role {
                StorageRole::SharedRun => 0,
                _ => index as u64,
            })
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
