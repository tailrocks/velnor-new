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

    pub(in crate::journal::connection::reservation::storage_budget) const fn as_byte(self) -> u8 {
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

    pub(in crate::journal::connection::reservation::storage_budget) const fn as_byte(self) -> u8 {
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

    pub(in crate::journal::connection::reservation::storage_budget) const fn as_byte(self) -> u8 {
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

/// This type defines the required mounts for each storage role.
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

/// This type names the mount namespace that supplies one mount record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum StorageObserver {
    /// Runner container.
    Runner,
    /// `DinD` container.
    Dind,
    /// Trusted host resource probe.
    Probe,
}

/// This type defines one mount location for a physical storage role.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct StorageAttachmentContract {
    /// Charged physical role.
    pub(crate) role: StorageRole,
    /// Namespace that reports the mount.
    pub(crate) observer: StorageObserver,
    /// This value gives the required mount target in that namespace.
    pub(crate) target: &'static str,
    /// Required mount access mode.
    pub(crate) read_only: bool,
    /// Host source for a probe-only attachment.
    pub(crate) source_target: Option<&'static str>,
    /// This value gives the path inside the named volume.
    pub(crate) volume_subpath: Option<&'static str>,
}
