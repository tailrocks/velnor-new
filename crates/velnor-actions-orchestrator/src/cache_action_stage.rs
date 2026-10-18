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
