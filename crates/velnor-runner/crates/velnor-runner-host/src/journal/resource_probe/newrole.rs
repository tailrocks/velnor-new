//! Closed helper roles and resource-probe request purposes.

use crate::error::HostError;

/// One helper role on the selected Docker engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HostHelperRole {
    /// Collect capacity or owned-cgroup facts.
    ResourceProbe,
    /// Publish one source-proof control file.
    ExecutionPublication,
}

impl HostHelperRole {
    pub(in crate::journal) fn parse(value: &str) -> Result<Self, HostError> {
        match value {
            "resource-probe" => Ok(Self::ResourceProbe),
            "execution-publication" => Ok(Self::ExecutionPublication),
            _ => Err(HostError::Journal),
        }
    }
}

/// One bounded request handled by the resource-probe helper.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ResourceProbePurpose {
    /// Measure selected guest capacity.
    Capacity,
    /// Measure cgroup limits for an owned worker pair.
    OwnedCgroups,
    /// Check that one recorded worker has no remaining references.
    CleanupObserver,
}

impl ResourceProbePurpose {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Capacity => "capacity",
            Self::OwnedCgroups => "owned-cgroups",
            Self::CleanupObserver => "cleanup-observer",
        }
    }

    pub(in crate::journal) fn parse(value: &str) -> Result<Self, HostError> {
        match value {
            "capacity" => Ok(Self::Capacity),
            "owned-cgroups" => Ok(Self::OwnedCgroups),
            "cleanup-observer" => Ok(Self::CleanupObserver),
            _ => Err(HostError::Journal),
        }
    }
}
