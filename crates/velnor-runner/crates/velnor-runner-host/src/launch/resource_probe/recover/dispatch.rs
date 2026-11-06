// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::{
    error::HostError,
    journal::{HostHelperRole, ProbeWork},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ActiveRecoveryTarget {
    ResourceProbe,
    ExecutionPublication,
}

pub(super) fn active_recovery_target(
    helper_role: HostHelperRole,
    work: &ProbeWork,
) -> Result<ActiveRecoveryTarget, HostError> {
    match (helper_role, work) {
        (HostHelperRole::ResourceProbe, ProbeWork::ResourceProbe { .. }) => {
            Ok(ActiveRecoveryTarget::ResourceProbe)
        }
        (HostHelperRole::ExecutionPublication, ProbeWork::ExecutionPublication) => {
            Ok(ActiveRecoveryTarget::ExecutionPublication)
        }
        _ => Err(HostError::Ownership),
    }
}
