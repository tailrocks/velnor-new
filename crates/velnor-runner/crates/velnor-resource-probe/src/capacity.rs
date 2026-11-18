// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[cfg(any(target_os = "linux", test))]
use crate::ProbeError;

#[cfg(any(target_os = "linux", test))]
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

#[cfg(any(target_os = "linux", test))]
pub(crate) struct StatvfsCounts {
    pub(crate) blocks: u64,
    pub(crate) bfree: u64,
    pub(crate) bavail: u64,
    pub(crate) frsize: u64,
}

#[cfg(any(target_os = "linux", test))]
pub(crate) fn record_free_bytes(counts: &StatvfsCounts) -> Result<u64, ProbeError> {
    let (_, _, available) =
        docker_root_capacity(counts.blocks, counts.bfree, counts.bavail, counts.frsize)?;
    Ok(available)
}

#[cfg(test)]
mod tests {
    use super::*;

    const GIB: u64 = 1024 * 1024 * 1024;

    #[test]
    fn record_free_bytes_is_what_unprivileged_users_can_claim() -> Result<(), ProbeError> {
        let counts = StatvfsCounts {
            blocks: 100 * GIB / 4096,
            bfree: 12 * GIB / 4096,
            bavail: 7 * GIB / 4096,
            frsize: 4096,
        };
        assert_eq!(record_free_bytes(&counts)?, 7 * GIB);
        Ok(())
    }
}
