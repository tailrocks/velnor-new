// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

pub(crate) fn checked_guest_capacity(
    worker_cpu_millicores: u64,
    worker_memory_bytes: u64,
    operational_cpu_millicores: u64,
    operational_memory_bytes: u64,
) -> Option<(u64, u64)> {
    Some((
        worker_cpu_millicores.checked_add(operational_cpu_millicores)?,
        worker_memory_bytes.checked_add(operational_memory_bytes)?,
    ))
}

pub(super) fn guest_capacity_fits(
    observed_cpu_millicores: u64,
    observed_memory_bytes: u64,
    required_cpu_millicores: u64,
    required_memory_bytes: u64,
) -> bool {
    observed_cpu_millicores >= required_cpu_millicores
        && observed_memory_bytes >= required_memory_bytes
}

pub(super) fn required_available_memory(
    held_memory_bytes: u64,
    request_memory_bytes: u64,
    retained_memory_bytes: u64,
    operational_reserve_bytes: u64,
) -> Option<u64> {
    held_memory_bytes
        .checked_add(request_memory_bytes)?
        .checked_add(retained_memory_bytes)?
        .checked_add(operational_reserve_bytes)
}
