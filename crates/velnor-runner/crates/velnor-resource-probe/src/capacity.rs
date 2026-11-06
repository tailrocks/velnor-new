// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[cfg(target_os = "linux")]
use crate::ProbeError;

#[cfg(target_os = "linux")]
pub(crate) fn docker_root_capacity(
    total_blocks: u64,
    free_blocks: u64,
    available_blocks: u64,
    fragment_size: u64,
) -> Result<(u64, u64, u64), ProbeError> {
    if fragment_size == 0 {
        return Err(ProbeError::Invalid("docker_root_fragment_size"));
    }
    let total = total_blocks
        .checked_mul(fragment_size)
        .ok_or(ProbeError::Overflow("docker_root_total"))?;
    let free = free_blocks
        .checked_mul(fragment_size)
        .ok_or(ProbeError::Overflow("docker_root_free"))?;
    let available = available_blocks
        .checked_mul(fragment_size)
        .ok_or(ProbeError::Overflow("docker_root_available"))?;
    if free > total || available > free {
        return Err(ProbeError::Invalid("docker_root_capacity"));
    }
    Ok((free, total, available))
}
