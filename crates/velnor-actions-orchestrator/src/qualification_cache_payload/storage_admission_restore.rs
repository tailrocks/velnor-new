use super::measure::{CacheTreeUsage, FreshCapacity};
use super::{MAX_OPERATION_BYTES, MAX_TRANSPORT_BYTES, VerifiedCacheBudget};

const MIN_STAGE_BYTES: u64 = 34 + 1_024;
const TAR_BLOCK_BYTES: u64 = 512;
const MIN_STAGE_INODES: u64 = 2;
const FIXED_SCRATCH_INODES: u64 = 8;
// One PAX path record, two member headers, block padding, and end markers.
// `tar_write::write_single_member` caps the effective path at 4,096 bytes.
const ACTION_TAR_OVERHEAD_BYTES: u64 = 8 * 1_024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::qualification_cache_payload) struct CacheRestoreLimits {
    download_limit: u64,
    raw_stage_limit: u64,
    live_growth: u64,
    live_entries: u64,
}

impl CacheRestoreLimits {
    pub(in crate::qualification_cache_payload) const fn max_download_bytes(self) -> u64 {
        self.download_limit
    }

    pub(in crate::qualification_cache_payload) const fn raw_stage_bytes(self) -> u64 {
        self.raw_stage_limit
    }

    pub(in crate::qualification_cache_payload) const fn live_growth_bytes(self) -> u64 {
        self.live_growth
    }

    pub(in crate::qualification_cache_payload) const fn live_growth_inodes(self) -> u64 {
        self.live_entries
    }
}

impl VerifiedCacheBudget {
    /// Bound SDK restore and useful-state import before archive download.
    ///
    /// The calculation includes the compressed archive, decoded one-file TAR,
    /// payload spool, retained stage file, and worst-case live-root allocation.
    /// Compressed archive size stays independent from raw stage length.
    pub(in crate::qualification_cache_payload) fn restore_limits(
        &self,
        requested_raw_ceiling: u64,
        compressed_archive_bytes: u64,
        current_live: CacheTreeUsage,
        current_stage: CacheTreeUsage,
        fresh: FreshCapacity,
    ) -> Result<CacheRestoreLimits, &'static str> {
        if compressed_archive_bytes == 0 || fresh.fragment_bytes == 0 {
            return Err("cache_storage_capacity_invalid");
        }
        if current_live.bytes > self.live_bytes || current_live.inodes > self.live_inodes {
            return Err("cache_storage_live_budget_exceeded");
        }
        if current_stage.bytes > self.stage_bytes || current_stage.inodes > self.stage_inodes {
            return Err("cache_storage_stage_budget_exceeded");
        }
        let live_remaining_bytes = self.live_bytes - current_live.bytes;
        let live_remaining_inodes = self.live_inodes - current_live.inodes;
        let stage_remaining_bytes = self.stage_bytes - current_stage.bytes;
        let stage_remaining_inodes = self.stage_inodes - current_stage.inodes;
        if stage_remaining_inodes < MIN_STAGE_INODES {
            return Err("cache_storage_inode_budget_exceeded");
        }

        let raw_ceiling = requested_raw_ceiling
            .min(MAX_OPERATION_BYTES)
            .min(MAX_TRANSPORT_BYTES - 34)
            .min(stage_remaining_bytes);
        if raw_ceiling < MIN_STAGE_BYTES {
            return Err("cache_storage_stage_budget_exceeded");
        }
        let download_stage_ceiling = stage_remaining_bytes
            .checked_sub(ACTION_TAR_OVERHEAD_BYTES)
            .and_then(|remaining| remaining.checked_sub(MIN_STAGE_BYTES * 3))
            .ok_or("cache_storage_restore_capacity_insufficient")?;
        let max_download_bytes = MAX_TRANSPORT_BYTES
            .min(MAX_OPERATION_BYTES)
            .min(download_stage_ceiling)
            .min(fresh.available_bytes.saturating_sub(peak_bytes(
                0,
                MIN_STAGE_BYTES,
                fresh.fragment_bytes,
            )?));
        if compressed_archive_bytes > max_download_bytes {
            return Err("cache_storage_download_budget_exceeded");
        }
        let minimum_peak = peak_bytes(
            compressed_archive_bytes,
            MIN_STAGE_BYTES,
            fresh.fragment_bytes,
        )?;
        if scratch_bytes(compressed_archive_bytes, MIN_STAGE_BYTES)? > stage_remaining_bytes
            || minimum_peak > fresh.available_bytes
            || live_growth(MIN_STAGE_BYTES, fresh.fragment_bytes)? > live_remaining_bytes
            || live_entry_count(MIN_STAGE_BYTES)? > live_remaining_inodes
            || live_entry_count(MIN_STAGE_BYTES)?
                .checked_add(FIXED_SCRATCH_INODES)
                .is_none_or(|required| {
                    fresh
                        .available_inodes
                        .is_some_and(|available| required > available)
                })
        {
            return Err("cache_storage_restore_capacity_insufficient");
        }

        let mut low = MIN_STAGE_BYTES;
        let mut high = raw_ceiling;
        while low < high {
            let middle = low + (high - low).div_ceil(2);
            if restore_fits(
                compressed_archive_bytes,
                middle,
                stage_remaining_bytes,
                live_remaining_bytes,
                live_remaining_inodes,
                fresh,
            )? {
                low = middle;
            } else {
                high = middle - 1;
            }
        }
        Ok(CacheRestoreLimits {
            download_limit: max_download_bytes,
            raw_stage_limit: low,
            live_growth: live_growth(low, fresh.fragment_bytes)?,
            live_entries: live_entry_count(low)?,
        })
    }
}

fn restore_fits(
    compressed_archive_bytes: u64,
    raw_stage_bytes: u64,
    stage_remaining_bytes: u64,
    live_remaining_bytes: u64,
    live_remaining_inodes: u64,
    fresh: FreshCapacity,
) -> Result<bool, &'static str> {
    let growth = live_growth(raw_stage_bytes, fresh.fragment_bytes)?;
    let inodes = live_entry_count(raw_stage_bytes)?
        .checked_add(FIXED_SCRATCH_INODES)
        .ok_or("cache_storage_capacity_invalid")?;
    Ok(
        scratch_bytes(compressed_archive_bytes, raw_stage_bytes)? <= stage_remaining_bytes
            && growth <= live_remaining_bytes
            && live_entry_count(raw_stage_bytes)? <= live_remaining_inodes
            && peak_bytes(
                compressed_archive_bytes,
                raw_stage_bytes,
                fresh.fragment_bytes,
            )? <= fresh.available_bytes
            && fresh
                .available_inodes
                .is_none_or(|available| inodes <= available),
    )
}

fn scratch_bytes(compressed_archive_bytes: u64, raw_stage_bytes: u64) -> Result<u64, &'static str> {
    compressed_archive_bytes
        .checked_add(ACTION_TAR_OVERHEAD_BYTES)
        .and_then(|value| value.checked_add(raw_stage_bytes.checked_mul(3)?))
        .ok_or("cache_storage_capacity_invalid")
}

fn live_growth(raw_stage_bytes: u64, fragment_bytes: u64) -> Result<u64, &'static str> {
    let entries = live_entry_count(raw_stage_bytes)?;
    raw_stage_bytes
        .checked_add(
            entries
                .checked_add(2)
                .and_then(|count| count.checked_mul(fragment_bytes))
                .ok_or("cache_storage_capacity_invalid")?,
        )
        .ok_or("cache_storage_capacity_invalid")
}

fn live_entry_count(raw_stage_bytes: u64) -> Result<u64, &'static str> {
    raw_stage_bytes
        .checked_add(TAR_BLOCK_BYTES - 1)
        .map(|value| value / TAR_BLOCK_BYTES)
        .and_then(|value| value.checked_add(4))
        .ok_or("cache_storage_capacity_invalid")
}

fn peak_bytes(
    compressed_archive_bytes: u64,
    raw_stage_bytes: u64,
    fragment_bytes: u64,
) -> Result<u64, &'static str> {
    let stage_scratch = scratch_bytes(compressed_archive_bytes, raw_stage_bytes)?;
    let growth = live_growth(raw_stage_bytes, fragment_bytes)?;
    let retained_stage_and_import = compressed_archive_bytes
        .checked_add(raw_stage_bytes)
        .and_then(|value| value.checked_add(growth))
        .ok_or("cache_storage_capacity_invalid")?;
    Ok(stage_scratch.max(retained_stage_and_import))
}
