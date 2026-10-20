use crate::error::HostError;

mod attachments;
mod types;
pub(crate) use types::{
    StorageAttachmentContract, StorageFilesystemKind, StorageInodePolicy, StorageMedium,
    StorageObserver, StorageRole, StorageRoleContract,
};

impl StorageRole {
    /// The eleven charged filesystem roles. Cache roles are aliases.
    pub(crate) const PHYSICAL: [Self; 11] = [
        Self::RunnerRoot,
        Self::RunnerHome,
        Self::RunnerWork,
        Self::RunnerTmp,
        Self::RunnerShm,
        Self::SharedRun,
        Self::DindRoot,
        Self::DindData,
        Self::DindTmp,
        Self::DindShm,
        Self::EngineMetadata,
    ];

    pub(crate) const ALL: [Self; 13] = [
        Self::RunnerRoot,
        Self::RunnerHome,
        Self::RunnerWork,
        Self::RunnerTmp,
        Self::RunnerShm,
        Self::SharedRun,
        Self::DindRoot,
        Self::DindData,
        Self::DindTmp,
        Self::DindShm,
        Self::CacheLive,
        Self::CacheStage,
        Self::EngineMetadata,
    ];

    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::RunnerRoot => "runner-root",
            Self::RunnerHome => "runner-home",
            Self::RunnerWork => "runner-work",
            Self::RunnerTmp => "runner-tmp",
            Self::RunnerShm => "runner-shm",
            Self::SharedRun => "shared-run",
            Self::DindRoot => "dind-root",
            Self::DindData => "dind-data",
            Self::DindTmp => "dind-tmp",
            Self::DindShm => "dind-shm",
            Self::CacheLive => "cache-live",
            Self::CacheStage => "cache-stage",
            Self::EngineMetadata => "engine-metadata",
        }
    }

    pub(crate) fn parse(value: &str) -> Result<Self, HostError> {
        match value {
            "runner-root" => Ok(Self::RunnerRoot),
            "runner-home" => Ok(Self::RunnerHome),
            "runner-work" => Ok(Self::RunnerWork),
            "runner-tmp" => Ok(Self::RunnerTmp),
            "runner-shm" => Ok(Self::RunnerShm),
            "shared-run" => Ok(Self::SharedRun),
            "dind-root" => Ok(Self::DindRoot),
            "dind-data" => Ok(Self::DindData),
            "dind-tmp" => Ok(Self::DindTmp),
            "dind-shm" => Ok(Self::DindShm),
            "cache-live" => Ok(Self::CacheLive),
            "cache-stage" => Ok(Self::CacheStage),
            "engine-metadata" => Ok(Self::EngineMetadata),
            _ => Err(HostError::Journal),
        }
    }

    /// Return the fixed production topology for one physical attachment.
    pub(crate) const fn contract(self) -> Option<StorageRoleContract> {
        use StorageMedium::{Disk, Tmpfs};
        use StorageObserver::{Dind, Probe, Runner};

        match self {
            Self::RunnerRoot => Some(storage_role_contract(Disk, "/", true, Runner, None)),
            Self::RunnerHome => Some(storage_role_contract(
                Tmpfs,
                "/home/runner",
                false,
                Runner,
                None,
            )),
            Self::RunnerWork => Some(storage_role_contract(
                Tmpfs,
                "/home/runner/_work",
                false,
                Runner,
                None,
            )),
            Self::RunnerTmp => Some(storage_role_contract(Tmpfs, "/tmp", false, Runner, None)),
            Self::RunnerShm => Some(storage_role_contract(
                Tmpfs, "/dev/shm", false, Runner, None,
            )),
            Self::SharedRun => Some(storage_role_contract(Tmpfs, "/run", false, Runner, None)),
            Self::DindRoot => Some(storage_role_contract(Disk, "/", true, Dind, None)),
            Self::DindData => Some(storage_role_contract(
                Tmpfs,
                "/var/lib/docker",
                false,
                Dind,
                None,
            )),
            Self::DindTmp => Some(storage_role_contract(Tmpfs, "/tmp", false, Dind, None)),
            Self::DindShm => Some(storage_role_contract(Tmpfs, "/dev/shm", false, Dind, None)),
            Self::EngineMetadata => Some(storage_role_contract(
                Disk,
                "/velnor/docker-root",
                true,
                Probe,
                Some("/var/lib/docker"),
            )),
            Self::CacheLive | Self::CacheStage => None,
        }
    }

    /// Return every namespace attachment required by this role.
    pub(crate) const fn attachments(self) -> &'static [StorageAttachmentContract] {
        match self {
            Self::RunnerRoot => &attachments::RUNNER_ROOT,
            Self::RunnerHome => &attachments::RUNNER_HOME,
            Self::RunnerWork => &attachments::RUNNER_WORK,
            Self::RunnerTmp => &attachments::RUNNER_TMP,
            Self::RunnerShm => &attachments::RUNNER_SHM,
            Self::SharedRun => &attachments::SHARED_RUN,
            Self::DindRoot => &attachments::DIND_ROOT,
            Self::DindData => &attachments::DIND_DATA,
            Self::DindTmp => &attachments::DIND_TMP,
            Self::DindShm => &attachments::DIND_SHM,
            Self::EngineMetadata => &attachments::ENGINE_METADATA,
            Self::CacheLive | Self::CacheStage => &[],
        }
    }
}

const fn storage_role_contract(
    medium: StorageMedium,
    target: &'static str,
    read_only: bool,
    observer: StorageObserver,
    source_target: Option<&'static str>,
) -> StorageRoleContract {
    StorageRoleContract {
        medium,
        target,
        read_only,
        observer,
        source_target,
    }
}

const fn attachment(
    role: StorageRole,
    observer: StorageObserver,
    target: &'static str,
    read_only: bool,
    source_target: Option<&'static str>,
) -> StorageAttachmentContract {
    StorageAttachmentContract {
        role,
        observer,
        target,
        read_only,
        source_target,
        volume_subpath: None,
    }
}

const fn attachment_with_subpath(
    role: StorageRole,
    observer: StorageObserver,
    target: &'static str,
    read_only: bool,
    source_target: Option<&'static str>,
    volume_subpath: Option<&'static str>,
) -> StorageAttachmentContract {
    StorageAttachmentContract {
        role,
        observer,
        target,
        read_only,
        source_target,
        volume_subpath,
    }
}

#[cfg(test)]
mod tests {
    use super::{StorageObserver, StorageRole};

    #[test]
    fn metadata_and_home_roles_bind_control_and_inbox_to_their_volumes() {
        let attachments = StorageRole::EngineMetadata.attachments();
        let control = attachments
            .iter()
            .find(|attachment| attachment.target == "/opt/velnor/source-proof/control")
            .expect("control attachment");
        let home_attachments = StorageRole::RunnerHome.attachments();
        let inbox = home_attachments
            .iter()
            .find(|attachment| attachment.target == "/opt/velnor/source-proof/inbox")
            .expect("inbox attachment");

        assert_eq!(control.observer, StorageObserver::Runner);
        assert!(control.read_only);
        assert_eq!(control.volume_subpath, Some("source-proof/control"));
        assert_eq!(inbox.role, StorageRole::RunnerHome);
        assert_eq!(inbox.observer, StorageObserver::Runner);
        assert!(!inbox.read_only);
        assert_eq!(inbox.volume_subpath, Some("source-proof/inbox"));
        assert_eq!(attachments.len(), 6);
        for (target, subpath) in [
            ("/.velnor-runtime-identity", "runtime-identity"),
            ("/opt/velnor/job-facts", "job-facts"),
        ] {
            assert!(attachments.iter().any(|attachment| {
                attachment.target == target
                    && attachment.observer == StorageObserver::Dind
                    && attachment.read_only
                    && attachment.volume_subpath == Some(subpath)
            }));
        }
        for (target, subpath) in [
            ("/.velnor-runtime-identity", "runtime-identity"),
            ("/opt/velnor/job-facts", "job-facts"),
        ] {
            assert!(attachments.iter().any(|attachment| {
                attachment.target == target && attachment.volume_subpath == Some(subpath)
            }));
        }
        assert_eq!(home_attachments.len(), 3);
    }
}
