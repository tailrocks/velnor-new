// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::HostError;

use super::super::plan::StorageReservationPlan;

pub(in crate::journal::connection::reservation::storage_budget) fn plan_memory(
    plan: &StorageReservationPlan,
) -> Result<u64, HostError> {
    plan.roles().iter().try_fold(0_u64, |sum, role| {
        sum.checked_add(role.retained_memory_bytes())
            .ok_or(HostError::Journal)
    })
}
