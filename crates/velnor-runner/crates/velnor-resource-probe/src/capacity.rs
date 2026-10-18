use crate::ProbeError;

/// Convert `statvfs` block counts to bytes and reject impossible capacity data.
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
