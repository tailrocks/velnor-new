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

impl StorageRole {
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
}
