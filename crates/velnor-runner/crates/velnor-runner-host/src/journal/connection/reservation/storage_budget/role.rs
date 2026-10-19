use crate::error::HostError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum StorageMedium {
    Disk,
    Tmpfs,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum StorageInodePolicy {
    Finite,
    DynamicBtrfs,
    Unknown,
}

impl StorageInodePolicy {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Finite => "finite",
            Self::DynamicBtrfs => "dynamic-btrfs",
            Self::Unknown => "unknown",
        }
    }

    pub(super) const fn as_byte(self) -> u8 {
        match self {
            Self::Finite => 1,
            Self::DynamicBtrfs => 2,
            Self::Unknown => 3,
        }
    }

    pub(crate) fn parse(value: &str) -> Result<Self, HostError> {
        match value {
            "finite" => Ok(Self::Finite),
            "dynamic-btrfs" => Ok(Self::DynamicBtrfs),
            "unknown" => Ok(Self::Unknown),
            _ => Err(HostError::Journal),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum StorageFilesystemKind {
    Btrfs,
    Tmpfs,
    Other,
}

impl StorageFilesystemKind {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Btrfs => "btrfs",
            Self::Tmpfs => "tmpfs",
            Self::Other => "other",
        }
    }

    pub(super) const fn as_byte(self) -> u8 {
        match self {
            Self::Btrfs => 1,
            Self::Tmpfs => 2,
            Self::Other => 3,
        }
    }

    pub(crate) fn parse(value: &str) -> Result<Self, HostError> {
        match value {
            "btrfs" => Ok(Self::Btrfs),
            "tmpfs" => Ok(Self::Tmpfs),
            "other" => Ok(Self::Other),
            _ => Err(HostError::Journal),
        }
    }
}

impl StorageMedium {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Disk => "disk",
            Self::Tmpfs => "tmpfs",
        }
    }

    pub(super) const fn as_byte(self) -> u8 {
        match self {
            Self::Disk => 1,
            Self::Tmpfs => 2,
        }
    }

    pub(crate) fn parse(value: &str) -> Result<Self, HostError> {
        match value {
            "disk" => Ok(Self::Disk),
            "tmpfs" => Ok(Self::Tmpfs),
            _ => Err(HostError::Journal),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum StorageRole {
    RunnerRoot,
    RunnerHome,
    RunnerWork,
    RunnerTmp,
    RunnerShm,
    SharedRun,
    DindRoot,
    DindData,
    DindTmp,
    DindShm,
    CacheLive,
    CacheStage,
    EngineMetadata,
}

/// Fixed mount requirements for one storage role.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct StorageRoleContract {
    /// Backing medium selected by the host policy.
    pub(crate) medium: StorageMedium,
    /// Path inside the observing container, or the probe bind target.
    pub(crate) target: &'static str,
    /// Whether the mount must be read-only.
    pub(crate) read_only: bool,
    /// Observer that supplies mount-namespace evidence.
    pub(crate) observer: StorageObserver,
    /// Host-side source path for the probe-only `EngineMetadata` role.
    pub(crate) source_target: Option<&'static str>,
}

/// Mount namespace that supplies one mount record for inspection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum StorageObserver {
    /// Runner container.
    Runner,
    /// `DinD` container.
    Dind,
    /// Trusted host resource probe.
    Probe,
}

/// One required mount location for a physical storage role.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct StorageAttachmentContract {
    /// Charged physical role.
    pub(crate) role: StorageRole,
    /// Namespace that reports the mount.
    pub(crate) observer: StorageObserver,
    /// Exact mount target in that namespace.
    pub(crate) target: &'static str,
    /// Required mount access mode.
    pub(crate) read_only: bool,
    /// Host source for a probe-only attachment.
    pub(crate) source_target: Option<&'static str>,
    /// Exact named-volume subpath for an attachment.
    pub(crate) volume_subpath: Option<&'static str>,
}

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
        use StorageObserver::{Dind, Probe, Runner};

        const RUNNER_ROOT: [StorageAttachmentContract; 1] =
            [attachment(StorageRole::RunnerRoot, Runner, "/", true, None)];
        const RUNNER_HOME: [StorageAttachmentContract; 3] = [
            attachment(StorageRole::RunnerHome, Runner, "/home/runner", false, None),
            attachment(StorageRole::RunnerHome, Dind, "/home/runner", false, None),
            attachment_with_subpath(
                StorageRole::RunnerHome,
                Runner,
                "/opt/velnor/source-proof/inbox",
                false,
                None,
                Some("source-proof/inbox"),
            ),
        ];
        const RUNNER_WORK: [StorageAttachmentContract; 2] = [
            attachment(
                StorageRole::RunnerWork,
                Runner,
                "/home/runner/_work",
                false,
                None,
            ),
            attachment(
                StorageRole::RunnerWork,
                Dind,
                "/home/runner/_work",
                false,
                None,
            ),
        ];
        const RUNNER_TMP: [StorageAttachmentContract; 1] = [attachment(
            StorageRole::RunnerTmp,
            Runner,
            "/tmp",
            false,
            None,
        )];
        const RUNNER_SHM: [StorageAttachmentContract; 1] = [attachment(
            StorageRole::RunnerShm,
            Runner,
            "/dev/shm",
            false,
            None,
        )];
        const SHARED_RUN: [StorageAttachmentContract; 2] = [
            attachment(StorageRole::SharedRun, Runner, "/run", false, None),
            attachment(StorageRole::SharedRun, Dind, "/run", false, None),
        ];
        const DIND_ROOT: [StorageAttachmentContract; 1] =
            [attachment(StorageRole::DindRoot, Dind, "/", true, None)];
        const DIND_DATA: [StorageAttachmentContract; 1] = [attachment(
            StorageRole::DindData,
            Dind,
            "/var/lib/docker",
            false,
            None,
        )];
        const DIND_TMP: [StorageAttachmentContract; 1] =
            [attachment(StorageRole::DindTmp, Dind, "/tmp", false, None)];
        const DIND_SHM: [StorageAttachmentContract; 1] = [attachment(
            StorageRole::DindShm,
            Dind,
            "/dev/shm",
            false,
            None,
        )];
        const ENGINE_METADATA: [StorageAttachmentContract; 4] = [
            attachment(
                StorageRole::EngineMetadata,
                Probe,
                "/velnor/docker-root",
                true,
                Some("/var/lib/docker"),
            ),
            attachment_with_subpath(
                StorageRole::EngineMetadata,
                Runner,
                "/.velnor-runtime-identity",
                true,
                None,
                Some("runtime-identity"),
            ),
            attachment_with_subpath(
                StorageRole::EngineMetadata,
                Runner,
                "/opt/velnor/job-facts",
                true,
                None,
                Some("job-facts"),
            ),
            attachment_with_subpath(
                StorageRole::EngineMetadata,
                Runner,
                "/opt/velnor/source-proof/control",
                true,
                None,
                Some("source-proof/control"),
            ),
        ];

        match self {
            Self::RunnerRoot => &RUNNER_ROOT,
            Self::RunnerHome => &RUNNER_HOME,
            Self::RunnerWork => &RUNNER_WORK,
            Self::RunnerTmp => &RUNNER_TMP,
            Self::RunnerShm => &RUNNER_SHM,
            Self::SharedRun => &SHARED_RUN,
            Self::DindRoot => &DIND_ROOT,
            Self::DindData => &DIND_DATA,
            Self::DindTmp => &DIND_TMP,
            Self::DindShm => &DIND_SHM,
            Self::EngineMetadata => &ENGINE_METADATA,
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
        assert_eq!(attachments.len(), 4);
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
