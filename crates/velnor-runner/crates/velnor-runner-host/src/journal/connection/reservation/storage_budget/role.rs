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

/// Fixed mount contract for a physical storage role.
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
    /// Host-side source path for the probe-only EngineMetadata role.
    pub(crate) source_target: Option<&'static str>,
}

/// Mount namespace that supplies one attachment witness.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum StorageObserver {
    /// Runner container.
    Runner,
    /// DinD container.
    Dind,
    /// Trusted host resource probe.
    Probe,
}

/// One required mount witness for a physical storage role.
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

        let contract = match self {
            Self::RunnerRoot => StorageRoleContract {
                medium: Disk,
                target: "/",
                read_only: true,
                observer: Runner,
                source_target: None,
            },
            Self::RunnerHome => StorageRoleContract {
                medium: Tmpfs,
                target: "/home/runner",
                read_only: false,
                observer: Runner,
                source_target: None,
            },
            Self::RunnerWork => StorageRoleContract {
                medium: Tmpfs,
                target: "/home/runner/_work",
                read_only: false,
                observer: Runner,
                source_target: None,
            },
            Self::RunnerTmp => StorageRoleContract {
                medium: Tmpfs,
                target: "/tmp",
                read_only: false,
                observer: Runner,
                source_target: None,
            },
            Self::RunnerShm => StorageRoleContract {
                medium: Tmpfs,
                target: "/dev/shm",
                read_only: false,
                observer: Runner,
                source_target: None,
            },
            Self::SharedRun => StorageRoleContract {
                medium: Tmpfs,
                target: "/run",
                read_only: false,
                observer: Runner,
                source_target: None,
            },
            Self::DindRoot => StorageRoleContract {
                medium: Disk,
                target: "/",
                read_only: true,
                observer: Dind,
                source_target: None,
            },
            Self::DindData => StorageRoleContract {
                medium: Tmpfs,
                target: "/var/lib/docker",
                read_only: false,
                observer: Dind,
                source_target: None,
            },
            Self::DindTmp => StorageRoleContract {
                medium: Tmpfs,
                target: "/tmp",
                read_only: false,
                observer: Dind,
                source_target: None,
            },
            Self::DindShm => StorageRoleContract {
                medium: Tmpfs,
                target: "/dev/shm",
                read_only: false,
                observer: Dind,
                source_target: None,
            },
            Self::EngineMetadata => StorageRoleContract {
                medium: Disk,
                target: "/velnor/docker-root",
                read_only: true,
                observer: Probe,
                source_target: Some("/var/lib/docker"),
            },
            Self::CacheLive | Self::CacheStage => return None,
        };
        Some(contract)
    }

    /// Return every namespace attachment required by this role.
    pub(crate) const fn attachments(self) -> &'static [StorageAttachmentContract] {
        use StorageObserver::{Dind, Probe, Runner};

        const RUNNER_ROOT: [StorageAttachmentContract; 1] = [attachment(
            StorageRole::RunnerRoot,
            Runner,
            "/",
            true,
            None,
        )];
        const RUNNER_HOME: [StorageAttachmentContract; 2] = [
            attachment(StorageRole::RunnerHome, Runner, "/home/runner", false, None),
            attachment(StorageRole::RunnerHome, Dind, "/home/runner", false, None),
        ];
        const RUNNER_WORK: [StorageAttachmentContract; 2] = [
            attachment(StorageRole::RunnerWork, Runner, "/home/runner/_work", false, None),
            attachment(StorageRole::RunnerWork, Dind, "/home/runner/_work", false, None),
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
        const DIND_ROOT: [StorageAttachmentContract; 1] = [attachment(
            StorageRole::DindRoot,
            Dind,
            "/",
            true,
            None,
        )];
        const DIND_DATA: [StorageAttachmentContract; 1] = [attachment(
            StorageRole::DindData,
            Dind,
            "/var/lib/docker",
            false,
            None,
        )];
        const DIND_TMP: [StorageAttachmentContract; 1] = [attachment(
            StorageRole::DindTmp,
            Dind,
            "/tmp",
            false,
            None,
        )];
        const DIND_SHM: [StorageAttachmentContract; 1] = [attachment(
            StorageRole::DindShm,
            Dind,
            "/dev/shm",
            false,
            None,
        )];
        const ENGINE_METADATA: [StorageAttachmentContract; 1] = [attachment(
            StorageRole::EngineMetadata,
            Probe,
            "/velnor/docker-root",
            true,
            Some("/var/lib/docker"),
        )];

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
    }
}
