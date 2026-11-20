// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::HostError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HostHelperRole {
    ResourceProbe,

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ResourceProbePurpose {
    Capacity,

    OwnedCgroups,

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
