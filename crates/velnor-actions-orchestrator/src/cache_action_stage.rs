//! Descriptor-bound staging for the one-file Actions cache transport.

mod compression;
mod context;
mod launcher;
mod receipt;
mod stage;
mod stage_parent;
mod tar;

#[cfg(test)]
mod tests;

pub use compression::CacheActionCompression;
pub use context::StageContext;
pub use launcher::{CACHE_TAR_LAUNCHER_INSTALL_OP, install_cache_tar_launchers};
pub use receipt::StageReceiptClaim;
pub use stage::{StageReader, StageWriter, create_stage, open_stage, rewrite_stage};
pub use tar::{StageAccess, TarOutcome, run_cache_tar};

/// Gives a stable error code for a rejected stage operation.
pub type StageError = &'static str;

/// This function makes sure that the stat value fits in an unsigned 64-bit integer.
pub(crate) fn checked_stat_u64<T>(value: T, error: StageError) -> Result<u64, StageError>
where
    T: TryInto<u64>,
{
    value.try_into().map_err(|_| error)
}

/// This function makes sure that the stat value fits in an unsigned 32-bit integer.
pub(crate) fn checked_stat_u32<T>(value: T, error: StageError) -> Result<u32, StageError>
where
    T: TryInto<u32>,
{
    value.try_into().map_err(|_| error)
}

#[cfg(test)]
mod stat_conversion_tests {
    use super::{checked_stat_u32, checked_stat_u64};

    #[test]
    fn checked_stat_values_support_native_widths_and_reject_negative_values() {
        assert_eq!(checked_stat_u64(17_u64, "invalid"), Ok(17));
        assert_eq!(checked_stat_u64(17_i32, "invalid"), Ok(17));
        assert_eq!(checked_stat_u64(-1_i32, "invalid"), Err("invalid"));
        assert_eq!(checked_stat_u32(0o755_u32, "invalid"), Ok(0o755));
        assert_eq!(checked_stat_u32(0o755_u16, "invalid"), Ok(0o755));
    }
}
