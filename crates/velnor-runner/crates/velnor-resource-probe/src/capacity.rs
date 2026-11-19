// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::ProbeError;

pub fn docker_root_capacity(
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

#[derive(Debug, Clone, Copy)]
pub struct StatvfsCounts {
    pub blocks: u64,
    pub bfree: u64,
    pub bavail: u64,
    pub frsize: u64,
}

pub fn record_free_bytes(counts: &StatvfsCounts) -> Result<u64, ProbeError> {
    let (_, _, available) =
        docker_root_capacity(counts.blocks, counts.bfree, counts.bavail, counts.frsize)?;
    Ok(available)
}
